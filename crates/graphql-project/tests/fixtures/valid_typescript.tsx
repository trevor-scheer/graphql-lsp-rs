import { gql } from '@apollo/client';

const GET_USER_QUERY = gql`
  query GetUser($id: ID!) {
    user(id: $id) {
      id
      name
      email
      posts {
        id
        title
        published
      }
    }
  }
`;

const USER_FRAGMENT = gql`
  fragment UserDetails on User {
    id
    name
    email
    createdAt
  }
`;

const GET_USERS_WITH_FRAGMENT = gql`
  query GetUsers($limit: Int) {
    users(limit: $limit) {
      ...UserDetails
      posts {
        id
        title
      }
    }
  }

  ${USER_FRAGMENT}
`;

export { GET_USER_QUERY, USER_FRAGMENT, GET_USERS_WITH_FRAGMENT };
