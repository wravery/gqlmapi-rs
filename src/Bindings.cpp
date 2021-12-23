// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

#include "gqlmapi-rs/include/Bindings.h"
#include "gqlmapi-rs/src/bindings.rs.h"

#include "MAPIGraphQL.h"

#include "graphqlservice/JSONResponse.h"

#include <iostream>
#include <memory>
#include <map>
#include <optional>
#include <queue>
#include <thread>

using namespace graphql;

class Subscription : public std::enable_shared_from_this<Subscription>
{
public:
	explicit Subscription(const std::shared_ptr<service::Request> &service,
						  rust::Box<NextContext> nextContext,
						  Bindings::NextCallback nextCallback,
						  rust::Box<CompleteContext> completeContext,
						  Bindings::CompleteCallback completeCallback) noexcept;
	~Subscription();

	void Subscribe(service::SubscriptionKey key) noexcept;
	void Unsubscribe();
	void Deliver(response::AwaitableValue &&payload);
	void Deliver(response::Value &&document);
	void Complete();

private:
	std::weak_ptr<service::Request> _weakService;
	rust::Box<NextContext> _nextContext;
	Bindings::NextCallback _nextCallback;
	rust::Box<CompleteContext> _completeContext;
	Bindings::CompleteCallback _completeCallback;
	std::optional<service::SubscriptionKey> _key = std::nullopt;
	bool _registered = false;
};

Subscription::Subscription(const std::shared_ptr<service::Request> &service,
						   rust::Box<NextContext> nextContext,
						   Bindings::NextCallback nextCallback,
						   rust::Box<CompleteContext> completeContext,
						   Bindings::CompleteCallback completeCallback) noexcept
	: _weakService{service}, _nextContext{std::move(nextContext)}, _nextCallback{std::move(nextCallback)}, _completeContext{std::move(completeContext)}, _completeCallback{std::move(completeCallback)}
{
}

Subscription::~Subscription()
{
	Unsubscribe();
}

void Subscription::Subscribe(service::SubscriptionKey key) noexcept
{
	_registered = true;
	_key = std::make_optional(key);
}

void Subscription::Unsubscribe()
{
	if (!_registered)
	{
		return;
	}

	_registered = false;

	auto deferUnsubscribe = std::move(_key);
	auto service = _weakService.lock();

	if (deferUnsubscribe && service)
	{
		service->unsubscribe({*deferUnsubscribe}).get();
		Complete();
	}
}

void Subscription::Deliver(response::AwaitableValue &&payload)
{
	response::Value document{response::Type::Map};

	try
	{
		document = payload.get();
	}
	catch (service::schema_exception &scx)
	{
		document.reserve(2);
		document.emplace_back(std::string{service::strData}, {});
		document.emplace_back(std::string{service::strErrors}, scx.getErrors());
	}
	catch (const std::exception &ex)
	{
		std::ostringstream oss;

		oss << "Caught exception delivering subscription payload: " << ex.what();
		document.reserve(2);
		document.emplace_back(std::string{service::strData}, {});
		document.emplace_back(std::string{service::strErrors}, response::Value{oss.str()});
	}

	Deliver(std::move(document));
}

void Subscription::Deliver(response::Value &&document)
{
	_nextContext = _nextCallback(std::move(_nextContext), rust::String{response::toJSON(std::move(document))});
}

void Subscription::Complete()
{
	_completeCallback(std::move(_completeContext));
}

class RegisteredSubscription
{
public:
	explicit RegisteredSubscription(const std::shared_ptr<service::Request> &service,
									peg::ast &ast,
									std::string_view operationName,
									response::Value &&variables,
									rust::Box<NextContext> nextContext,
									Bindings::NextCallback nextCallback,
									rust::Box<CompleteContext> completeContext,
									Bindings::CompleteCallback completeCallback);

	void Unsubscribe() noexcept;

private:
	std::shared_ptr<Subscription> _subscription;
};

RegisteredSubscription::RegisteredSubscription(const std::shared_ptr<service::Request> &service,
											   peg::ast &ast,
											   std::string_view operationName,
											   response::Value &&variables,
											   rust::Box<NextContext> nextContext,
											   Bindings::NextCallback nextCallback,
											   rust::Box<CompleteContext> completeContext,
											   Bindings::CompleteCallback completeCallback)
	: _subscription{std::make_shared<Subscription>(service,
												   std::move(nextContext),
												   std::move(nextCallback),
												   std::move(completeContext),
												   std::move(completeCallback))}
{
	if (service->findOperationDefinition(ast, operationName).first == service::strSubscription)
	{
		_subscription->Subscribe(
			service->subscribe({[weakSubscription = std::weak_ptr{_subscription}](response::Value payload)
								{
									auto subscription = weakSubscription.lock();

									if (subscription)
									{
										subscription->Deliver(std::move(payload));
									}
								},
								peg::ast{ast},
								std::string{operationName},
								std::move(variables)})
				.get());
	}
	else
	{
		_subscription->Deliver(service->resolve({ast,
												 operationName,
												 std::move(variables)}));
		_subscription->Complete();
	}
}

void RegisteredSubscription::Unsubscribe() noexcept
{
	const auto subscription = std::move(_subscription);

	if (subscription)
	{
		subscription->Unsubscribe();
	}
}

class Bindings::impl
{
public:
	impl() = default;
	~impl() = default;

	void startService(bool useDefaultProfile) noexcept;
	void stopService();

	std::int32_t parseQuery(std::string_view query);
	void discardQuery(std::int32_t queryId) noexcept;

	std::int32_t subscribe(std::int32_t queryId,
						   std::string_view operationName,
						   std::string_view variables,
						   rust::Box<NextContext> nextContext,
						   NextCallback nextCallback,
						   rust::Box<CompleteContext> completeContext,
						   CompleteCallback completeCallback);
	void unsubscribe(std::int32_t subscriptionId);

private:
	std::shared_ptr<service::Request> service;
	std::map<std::int32_t, peg::ast> queryMap;
	std::map<std::int32_t, std::unique_ptr<RegisteredSubscription>> subscriptionMap;
};

void Bindings::impl::startService(bool useDefaultProfile) noexcept
{
	service = mapi::GetService(useDefaultProfile);
}

void Bindings::impl::stopService()
{
	if (service)
	{
		for (const auto &entry : subscriptionMap)
		{
			entry.second->Unsubscribe();
		}

		subscriptionMap.clear();
		queryMap.clear();
		service.reset();
	}
}

std::int32_t Bindings::impl::parseQuery(std::string_view query)
{
	const std::int32_t queryId = (queryMap.empty() ? 1 : queryMap.crbegin()->first + 1);

	queryMap[queryId] = peg::parseString(query);
	return queryId;
}

void Bindings::impl::discardQuery(std::int32_t queryId) noexcept
{
	queryMap.erase(queryId);
}

std::int32_t Bindings::impl::subscribe(std::int32_t queryId,
									   std::string_view operationName,
									   std::string_view variables,
									   rust::Box<NextContext> nextContext,
									   NextCallback nextCallback,
									   rust::Box<CompleteContext> completeContext,
									   CompleteCallback completeCallback)
{
	const auto itrQuery = queryMap.find(queryId);

	if (itrQuery == queryMap.cend())
	{
		throw std::runtime_error("Unknown queryId");
	}

	auto &ast = itrQuery->second;
	auto parsedVariables = (variables.empty() ? response::Value(response::Type::Map) : response::parseJSON(std::string{variables}));

	if (parsedVariables.type() != response::Type::Map)
	{
		throw std::runtime_error("Invalid variables object");
	}

	if (!service)
	{
		throw std::runtime_error("Did not call startService");
	}

	const std::int32_t subscriptionId = (subscriptionMap.empty() ? 1 : subscriptionMap.crbegin()->first + 1);

	subscriptionMap[subscriptionId] = std::make_unique<RegisteredSubscription>(service,
																			   ast,
																			   operationName,
																			   std::move(parsedVariables),
																			   std::move(nextContext),
																			   std::move(nextCallback),
																			   std::move(completeContext),
																			   std::move(completeCallback));

	return subscriptionId;
}

void Bindings::impl::unsubscribe(std::int32_t subscriptionId)
{
	auto itr = subscriptionMap.find(subscriptionId);

	if (itr != subscriptionMap.end())
	{
		itr->second->Unsubscribe();
		subscriptionMap.erase(itr);
	}
}

Bindings::Bindings() noexcept
	: m_pimpl{std::make_unique<impl>()}
{
}

Bindings::~Bindings()
{
}

void Bindings::startService(bool useDefaultProfile) const noexcept
{
	m_pimpl->startService(useDefaultProfile);
}

void Bindings::stopService() const
{
	m_pimpl->stopService();
}

std::int32_t Bindings::parseQuery(rust::Str query) const
{
	return m_pimpl->parseQuery(std::string_view{query.data(), query.size()});
}

void Bindings::discardQuery(std::int32_t queryId) const noexcept
{
	m_pimpl->discardQuery(queryId);
}

std::int32_t Bindings::subscribe(std::int32_t queryId,
								 rust::Str operationName,
								 rust::Str variables,
								 rust::Box<NextContext> nextContext,
								 NextCallback nextCallback,
								 rust::Box<CompleteContext> completeContext,
								 CompleteCallback completeCallback) const
{
	return m_pimpl->subscribe(queryId,
							  std::string_view{operationName.data(), operationName.size()},
							  std::string_view{variables.data(), variables.size()},
							  std::move(nextContext),
							  std::move(nextCallback),
							  std::move(completeContext),
							  std::move(completeCallback));
}

void Bindings::unsubscribe(std::int32_t subscriptionId) const
{
	m_pimpl->unsubscribe(subscriptionId);
}

std::unique_ptr<Bindings> make_bindings() noexcept
{
	return std::make_unique<Bindings>();
}