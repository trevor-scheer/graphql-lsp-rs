import { gql } from "@apollo/client";

// This should be valid
const VALID_QUERY = gql`
  query GetUser($id: ID!) {
    user(id: $id) {
      id
      name
      posts {
        id
        title
      }
    }
  }
`;

// This should have errors
const INVALID_QUERY = gql`
  query GetUserWithInvalidFields($id: ID!) {
    user(id: $id) {
      id
      name
      invalidField
      email
      posts {
        id
        title
        anotherInvalidField
      }
      ...UserFragment
    }
  }
`;

const FRAGMENT = gql`
  fragment UserFragment on User {
    id
    name
    b
  }
`;
