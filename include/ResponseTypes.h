// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

#pragma once

#include "rust/cxx.h"

#include "graphqlservice/GraphQLResponse.h"

using ResponseValueType = graphql::response::Type;

struct ResponseMapEntry;

struct ResponseValue
{
    ResponseValue(ResponseValueType type);
    ResponseValue(graphql::response::Value &&other);

    ResponseValueType getType() const noexcept;

    ResponseValue &fromJson() noexcept;

    void reserve(size_t additional);
    bool pushMapEntry(rust::Str name, std::unique_ptr<ResponseValue> value);
    void pushListEntry(std::unique_ptr<ResponseValue> value);

    void setString(rust::Str value);
    void setBool(bool value);
    void setInt(std::int64_t value);
    void setFloat(double value);

    std::unique_ptr<std::vector<ResponseMapEntry>> releaseMap();
    std::unique_ptr<std::vector<ResponseValue>> releaseList();
    std::unique_ptr<std::string> releaseString();
    bool getBool() const;
    std::int64_t getInt() const;
    double getFloat() const;
    std::unique_ptr<ResponseValue> releaseScalar();

    graphql::response::Value releaseValue() noexcept;

private:
    graphql::response::Value m_impl;
};

std::unique_ptr<ResponseValue> makeResponseValue(ResponseValueType type) noexcept;

rust::String toJSON(graphql::response::Value&& response);
graphql::response::Value parseJSON(std::string_view document);