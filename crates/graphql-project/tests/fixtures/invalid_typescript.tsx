import { gql } from '@apollo/client';

const INVALID_QUERY = gql`
  query GetUser($id: ID!) {
    user(id: $id) {
      id
      invalidField
      name
      anotherBadField
    }
  }
`;

const MISSING_ARG_QUERY = gql`
  query GetPost {
    post {
      id
      title
      content
    }
  }
`;

export { INVALID_QUERY, MISSING_ARG_QUERY };
