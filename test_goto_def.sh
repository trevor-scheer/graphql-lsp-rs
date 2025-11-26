#!/bin/bash

# Simple test script for goto definition

cd "$(dirname "$0")"

# Create test files
mkdir -p /tmp/graphql-test
cat > /tmp/graphql-test/fragment.graphql <<EOF
fragment UserFields on User {
  id
  name
  email
}
EOF

cat > /tmp/graphql-test/query.graphql <<EOF
query GetUser {
  user {
    ...UserFields
  }
}
EOF

cat > /tmp/graphql-test/schema.graphql <<EOF
type User {
  id: ID!
  name: String!
  email: String!
}

type Query {
  user: User
}
EOF

cat > /tmp/graphql-test/graphql.config.yaml <<EOF
schema: schema.graphql
documents: "*.graphql"
EOF

echo "Test files created in /tmp/graphql-test/"
echo ""
echo "To test manually:"
echo "1. Open VSCode in /tmp/graphql-test/"
echo "2. Open query.graphql"
echo "3. Try goto definition on 'UserFields' (line 3)"
echo ""
echo "The fragment is defined in fragment.graphql:1"
