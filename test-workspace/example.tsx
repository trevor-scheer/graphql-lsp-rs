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
    }
  }
`;
import { gql } from '@apollo/client';
