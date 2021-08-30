// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

#pragma once

#include "rust/cxx.h"

#include <cstdint>
#include <memory>
#include <string>

struct NextContext;
struct CompleteContext;

class Bindings
{
public:
	Bindings() noexcept;
	~Bindings();

	void startService(bool useDefaultProfile) const noexcept;
	void stopService() const noexcept;

	std::int32_t parseQuery(rust::Str query) const;
	void discardQuery(std::int32_t queryId) const noexcept;

	using NextCallback = rust::Fn<rust::Box<NextContext>(rust::Box<NextContext>, rust::String)>;
	using CompleteCallback = rust::Fn<void(rust::Box<CompleteContext>)>;

	std::int32_t subscribe(std::int32_t queryId,
						   rust::Str operationName,
						   rust::Str variables,
						   rust::Box<NextContext> nextContext,
						   NextCallback nextCallback,
						   rust::Box<CompleteContext> completeContext,
						   CompleteCallback completeCallback) const;
	void unsubscribe(std::int32_t subscriptionId) const noexcept;

private:
	class impl;
	std::unique_ptr<impl> m_pimpl;
};

std::unique_ptr<Bindings> make_bindings() noexcept;