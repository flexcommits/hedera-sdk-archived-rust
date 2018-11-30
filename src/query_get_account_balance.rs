use crate::{
    proto::{self, Query::Query_oneof_query, QueryHeader::QueryHeader, ToProto},
    query::{Query, QueryInner},
    AccountId, Client, ErrorKind, PreCheckCode,
};
use failure::Error;

pub type QueryGetAccountBalanceResponse = u64;

pub struct QueryGetAccountBalance {
    account: AccountId,
}

impl QueryGetAccountBalance {
    pub fn new(client: &Client, account: AccountId) -> Query<QueryGetAccountBalanceResponse> {
        Query::new(client, Self { account })
    }
}

impl QueryInner for QueryGetAccountBalance {
    type Response = QueryGetAccountBalanceResponse;

    fn get(&self, mut response: proto::Response::Response) -> Result<Self::Response, Error> {
        let mut response = response.take_cryptogetAccountBalance();
        let header = response.take_header();

        match header.get_nodeTransactionPrecheckCode().into() {
            PreCheckCode::Ok => Ok(response.get_balance()),
            code => Err(ErrorKind::PreCheck(code))?,
        }
    }

    fn to_query_proto(&self, header: QueryHeader) -> Result<Query_oneof_query, Error> {
        let mut query = proto::CryptoGetAccountBalance::CryptoGetAccountBalanceQuery::new();
        query.set_header(header);
        query.set_accountID(self.account.to_proto()?);

        Ok(Query_oneof_query::cryptogetAccountBalance(query))
    }
}
