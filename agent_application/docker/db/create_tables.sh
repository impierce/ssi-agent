#!/bin/bash
aws dynamodb create-table \
  --table-name Events \
      --key-schema \
        AttributeName=AggregateTypeAndId,KeyType=HASH \
        AttributeName=AggregateIdSequence,KeyType=RANGE \
  --attribute-definitions \
        AttributeName=AggregateTypeAndId,AttributeType=S \
        AttributeName=AggregateIdSequence,AttributeType=N \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name connection \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name all_connections \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name document \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name all_documents \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name service \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name all_services \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name offer \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name all_offers \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name pre_authorized_code \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name access_token \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name credential \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name all_credentials \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name server_config \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name received_offer \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name all_received_offers \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name holder_credential \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name presentation \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name all_presentations \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name all_holder_credentials \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name authorization_request \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000

aws dynamodb create-table \
  --table-name all_authorization_requests \
      --key-schema \
        AttributeName=ViewId,KeyType=HASH \
  --attribute-definitions \
        AttributeName=ViewId,AttributeType=S \
  --billing-mode PAY_PER_REQUEST \
  --endpoint-url http://cqrs-dynamodb-db:8000
