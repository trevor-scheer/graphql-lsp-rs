import { gql } from '@apollo/client';

const USER_FIELDS = gql`
  fragment UserFieldsTS on User {
    id
    name
    email
  }
`;

const GET_USERS = gql`
  query GetUsersTS {
    users {
      ...UserFieldsTS
      posts {
        id
        title
      }
    }
  }
`;
