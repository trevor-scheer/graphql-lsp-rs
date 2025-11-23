import { gql } from '@apollo/client';

const QUERY_WITH_CLIENT_DIRECTIVE = gql`
  query GetUser($id: ID!) {
    user(id: $id) {
      id
      name
      email
      isLoggedIn @client
      localData @client(always: true)
    }
  }
`;

const QUERY_WITH_CONNECTION = gql`
  query GetPosts($after: String) {
    posts @connection(key: "allPosts", filter: ["published"]) {
      id
      title
      published
    }
  }
`;

const QUERY_WITH_DEFER = gql`
  query GetUserWithDefer($id: ID!) {
    user(id: $id) {
      id
      name
      ... @defer(label: "posts") {
        posts {
          id
          title
        }
      }
    }
  }
`;

export { QUERY_WITH_CLIENT_DIRECTIVE, QUERY_WITH_CONNECTION, QUERY_WITH_DEFER };
