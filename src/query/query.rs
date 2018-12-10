use crate::{
    proto::{
        self,
        CryptoService_grpc::{CryptoService, CryptoServiceClient},
        FileService_grpc::{FileService, FileServiceClient},
        Query::Query_oneof_query,
        QueryHeader::QueryHeader,
        SmartContractService_grpc::{SmartContractService, SmartContractServiceClient},
        ToProto,
    },
    transaction::{Transaction, TransactionCryptoTransfer},
    AccountId, Client, ErrorKind, PreCheckCode, SecretKey,
};
use failure::Error;
use std::{sync::Arc, thread::sleep, time::Duration};

#[doc(hidden)]
pub trait QueryInner {
    type Response;
    fn get(&self, response: proto::Response::Response) -> Result<Self::Response, Error>;
    fn to_query_proto(&self, header: QueryHeader) -> Result<Query_oneof_query, Error>;
}

pub struct Query<T> {
    crypto_service: Arc<CryptoServiceClient>,
    contract_service: Arc<SmartContractServiceClient>,
    file_service: Arc<FileServiceClient>,
    kind: proto::QueryHeader::ResponseType,
    payment: Option<proto::Transaction::Transaction>,
    secret: Option<Arc<SecretKey>>,
    operator: Option<AccountId>,
    node: Option<AccountId>,
    attempt: u64,
    inner: Box<dyn QueryInner<Response = T>>,
}

impl<T> Query<T> {
    pub(crate) fn new<U: QueryInner<Response = T> + 'static>(client: &Client, inner: U) -> Self {
        Self {
            kind: proto::QueryHeader::ResponseType::ANSWER_ONLY,
            payment: None,
            crypto_service: client.crypto.clone(),
            contract_service: client.contract.clone(),
            file_service: client.file.clone(),
            node: client.node,
            operator: client.operator,
            secret: client.operator_secret.clone(),
            attempt: 0,
            inner: Box::new(inner),
        }
    }

    pub fn payment<S: 'static>(
        &mut self,
        transaction: &mut Transaction<TransactionCryptoTransfer, S>,
    ) -> Result<&mut Self, Error> {
        self.payment = Some(transaction.build().take_raw()?.tx);
        Ok(self)
    }

    pub fn get(&mut self) -> Result<T, Error> {
        let mut response = self.send()?;
        let header = take_header(&mut response);
        match header.get_nodeTransactionPrecheckCode().into() {
            PreCheckCode::Ok => self.inner.get(response),

            PreCheckCode::Busy if self.attempt < 5 => {
                self.attempt += 1;
                sleep(Duration::from_secs(self.attempt * 2));
                self.get()
            }

            PreCheckCode::InvalidTransaction if self.payment.is_none() => {
                if self.operator.is_some() && self.node.is_some() && self.secret.is_some() {
                    let cost = header.get_cost();
                    let operator = self.operator;
                    let node = self.node;
                    let operator_secret = self.secret.clone();

                    self.payment = Some(
                        TransactionCryptoTransfer::new(&Client {
                            node,
                            operator,
                            operator_secret,
                            crypto: self.crypto_service.clone(),
                            file: self.file_service.clone(),
                            contract: self.contract_service.clone(),
                        })
                        .transfer(*node.as_ref().unwrap(), cost as i64)
                        .transfer(*operator.as_ref().unwrap(), -(cost as i64))
                        .build()
                        .take_raw()?
                        .tx,
                    );

                    // Wait 1s before trying again
                    sleep(Duration::from_secs(1));

                    self.get()
                } else {
                    // Requires monies and we don't have anything defaulted
                    // todo: return a more specific error
                    Err(ErrorKind::PreCheck(PreCheckCode::InvalidTransaction))?
                }
            }

            code => Err(ErrorKind::PreCheck(code))?,
        }
    }

    pub fn cost(&mut self) -> Result<u64, Error> {
        // NOTE: This isn't the most ideal way to switch response types..
        self.kind = proto::QueryHeader::ResponseType::COST_ANSWER;
        let mut response = self.send()?;

        let header = take_header(&mut response);
        match header.get_nodeTransactionPrecheckCode().into() {
            PreCheckCode::Ok | PreCheckCode::InvalidTransaction => Ok(header.get_cost()),

            PreCheckCode::Busy if self.attempt < 5 => {
                self.attempt += 1;
                sleep(Duration::from_secs(self.attempt * 2));
                self.cost()
            }

            code => Err(ErrorKind::PreCheck(code))?,
        }
    }

    fn send(&self) -> Result<proto::Response::Response, Error> {
        use self::proto::Query::Query_oneof_query::*;

        let query: proto::Query::Query = self.to_proto()?;
        log::trace!("sent: {:#?}", query);

        let o = grpc::RequestOptions::default();
        let response = match query.query {
            Some(cryptogetAccountBalance(_)) => self.crypto_service.crypto_get_balance(o, query),
            Some(transactionGetReceipt(_)) => {
                self.crypto_service.get_transaction_receipts(o, query)
            }
            Some(cryptoGetInfo(_)) => self.crypto_service.get_account_info(o, query),
            Some(fileGetInfo(_)) => self.file_service.get_file_info(o, query),
            Some(fileGetContents(_)) => self.file_service.get_file_content(o, query),
            Some(transactionGetRecord(_)) => self.crypto_service.get_tx_record_by_tx_id(o, query),
            Some(cryptoGetAccountRecords(_)) => self.crypto_service.get_account_records(o, query),
            Some(contractGetInfo(_)) => self.contract_service.get_contract_info(o, query),
            Some(contractGetBytecode(_)) => self.contract_service.contract_get_bytecode(o, query),

            _ => unreachable!(),
        };

        // TODO: Implement async
        let response = response.wait_drop_metadata()?;

        log::trace!("recv: {:#?}", response);

        Ok(response)
    }
}

impl<T> ToProto<proto::Query::Query> for Query<T> {
    fn to_proto(&self) -> Result<proto::Query::Query, Error> {
        let mut header = proto::QueryHeader::QueryHeader::new();
        header.set_responseType(self.kind);

        if let Some(payment) = &self.payment {
            header.set_payment(payment.clone());
        }

        let mut query = proto::Query::Query::new();
        query.query = Some(self.inner.to_query_proto(header)?);

        Ok(query)
    }
}

// this is needed because some times a query is responded to with the wrong
// envelope type when an error occurs; this ensures we can get the error
pub(crate) fn take_header(
    response: &mut proto::Response::Response,
) -> proto::ResponseHeader::ResponseHeader {
    use self::proto::Response::Response_oneof_response::*;

    match &mut response.response {
        Some(getByKey(ref mut res)) => res.take_header(),
        Some(getBySolidityID(ref mut res)) => res.take_header(),
        Some(contractCallLocal(ref mut res)) => res.take_header(),
        Some(contractGetBytecodeResponse(ref mut res)) => res.take_header(),
        Some(contractGetInfo(ref mut res)) => res.take_header(),
        Some(contractGetRecordsResponse(ref mut res)) => res.take_header(),
        Some(cryptogetAccountBalance(ref mut res)) => res.take_header(),
        Some(cryptoGetAccountRecords(ref mut res)) => res.take_header(),
        Some(cryptoGetInfo(ref mut res)) => res.take_header(),
        Some(cryptoGetClaim(ref mut res)) => res.take_header(),
        Some(cryptoGetProxyStakers(ref mut res)) => res.take_header(),
        Some(fileGetContents(ref mut res)) => res.take_header(),
        Some(fileGetInfo(ref mut res)) => res.take_header(),
        Some(transactionGetReceipt(ref mut res)) => res.take_header(),
        Some(transactionGetRecord(ref mut res)) => res.take_header(),

        _ => unreachable!(),
    }
}
