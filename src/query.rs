use std::sync::Arc;

use failure::Error;
use grpc::ClientStub;

use crate::{
    proto::{
        self, CryptoService_grpc::CryptoService, FileService_grpc::FileService,
        Query::Query_oneof_query, QueryHeader::QueryHeader,
        SmartContractService_grpc::SmartContractService, ToProto,
    },
    Client, ErrorKind, PreCheckCode,
};

// Re-export query-like things under the query namespace
pub use crate::{
    query_contract_get_bytecode::*, query_contract_get_info::*,
    query_crypto_get_account_balance::*, query_crypto_get_account_records::*,
    query_crypto_get_info::*, query_file_get_contents::*, query_file_get_info::*,
    query_get_transaction_receipt::*, query_transaction_get_record::*,
};

#[doc(hidden)]
pub trait QueryInner {
    type Response;
    fn get(&self, response: proto::Response::Response) -> Result<Self::Response, Error>;
    fn to_query_proto(&self, header: QueryHeader) -> Result<Query_oneof_query, Error>;
}

pub struct Query<T> {
    pub(crate) client: Arc<grpc::Client>,
    kind: proto::QueryHeader::ResponseType,
    // TODO: payment: Transaction,
    inner: Box<dyn QueryInner<Response = T>>,
}

impl<T> Query<T> {
    pub(crate) fn new<U: QueryInner<Response = T> + 'static>(client: &Client, inner: U) -> Self {
        Self {
            kind: proto::QueryHeader::ResponseType::ANSWER_ONLY,
            client: client.inner.clone(),
            inner: Box::new(inner),
        }
    }

    pub(crate) fn send(&self) -> Result<proto::Response::Response, Error> {
        use self::proto::{
            CryptoService_grpc::CryptoServiceClient, FileService_grpc::FileServiceClient,
            Query::Query_oneof_query::*, SmartContractService_grpc::SmartContractServiceClient,
        };

        let query: proto::Query::Query = self.to_proto()?;
        log::trace!("sent: {:#?}", query);

        let o = grpc::RequestOptions::default();

        let client = Arc::clone(&self.client);
        let response = match query.query {
            Some(cryptogetAccountBalance(_)) => {
                CryptoServiceClient::with_client(client).crypto_get_balance(o, query)
            }

            Some(transactionGetReceipt(_)) => {
                CryptoServiceClient::with_client(client).get_transaction_receipts(o, query)
            }

            Some(cryptoGetInfo(_)) => {
                CryptoServiceClient::with_client(client).get_account_info(o, query)
            }

            Some(fileGetInfo(_)) => FileServiceClient::with_client(client).get_file_info(o, query),

            Some(fileGetContents(_)) => {
                FileServiceClient::with_client(client).get_file_content(o, query)
            }

            Some(transactionGetRecord(_)) => {
                CryptoServiceClient::with_client(client).get_tx_record_by_tx_id(o, query)
            }

            Some(cryptoGetAccountRecords(_)) => {
                CryptoServiceClient::with_client(client).get_account_records(o, query)
            }

            Some(contractGetInfo(_)) => {
                SmartContractServiceClient::with_client(client).get_contract_info(o, query)
            }

            Some(contractGetBytecode(_)) => {
                SmartContractServiceClient::with_client(client).contract_get_bytecode(o, query)
            }

            _ => unreachable!(),
        };

        // TODO: Implement async
        let response = response.wait_drop_metadata()?;

        log::trace!("recv: {:#?}", response);

        Ok(response)
    }

    pub fn get(&mut self) -> Result<T, Error> {
        self.inner.get(self.send()?)
    }

    pub fn cost(&mut self) -> Result<u64, Error> {
        use self::proto::Response::Response_oneof_response::*;

        // NOTE: This isn't the most ideal way to switch response types..
        self.kind = proto::QueryHeader::ResponseType::COST_ANSWER;
        let response = self.send()?;

        // Why is the cost field inside the specific answer type field in the proto ?
        // Maybe send up a question later.

        let header = match response.response {
            Some(cryptogetAccountBalance(mut res)) => res.take_header(),
            Some(transactionGetReceipt(mut res)) => res.take_header(),
            Some(cryptoGetInfo(mut res)) => res.take_header(),
            Some(fileGetInfo(mut res)) => res.take_header(),
            Some(fileGetContents(mut res)) => res.take_header(),
            Some(transactionGetRecord(mut res)) => res.take_header(),
            Some(cryptoGetAccountRecords(mut res)) => res.take_header(),

            _ => unreachable!(),
        };

        match header.get_nodeTransactionPrecheckCode().into() {
            PreCheckCode::Ok | PreCheckCode::InvalidTransaction => Ok(header.get_cost()),
            code => Err(ErrorKind::PreCheck(code))?,
        }
    }
}

impl<T> ToProto<proto::Query::Query> for Query<T> {
    fn to_proto(&self) -> Result<proto::Query::Query, Error> {
        let mut header = proto::QueryHeader::QueryHeader::new();
        header.set_responseType(self.kind);

        let mut query = proto::Query::Query::new();
        query.query = Some(self.inner.to_query_proto(header)?);

        Ok(query)
    }
}
