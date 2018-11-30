use crate::{
    crypto::SecretKey,
    error::ErrorKind,
    proto::{self, CryptoService_grpc::CryptoService, ToProto},
    AccountId, Client, PreCheckCode, TransactionId,
};
use failure::Error;
use grpc::ClientStub;
use protobuf::{Message, RepeatedField};
use query_interface::Object;
use std::{any::Any, marker::PhantomData, sync::Arc, time::Duration};

// Re-export transaction like things under the transaction namespace
pub use crate::{
    transaction_admin_delete::*, transaction_admin_recover::*, transaction_crypto_create::*,
    transaction_crypto_delete::*, transaction_crypto_delete_claim::*,
    transaction_crypto_transfer::*, transaction_crypto_update::*,
    transaction_response::TransactionResponse,
};

pub struct Transaction<T> {
    id: Option<TransactionId>,
    client: Arc<grpc::Client>,
    node: Option<AccountId>,
    secrets: Vec<SecretKey>,
    memo: Option<String>,
    pub(crate) inner: Box<dyn Object>,
    phantom: PhantomData<T>,
}

impl<T: 'static> Transaction<T> {
    pub(crate) fn new(client: &Client, inner: T) -> Self
    where
        T: Object + ToProto<proto::Transaction::TransactionBody_oneof_data> + 'static,
    {
        let inner = Box::<T>::new(inner);
        Self {
            client: client.inner.clone(),
            id: None,
            node: None,
            memo: None,
            secrets: Vec::new(),
            inner: inner as Box<dyn Object>,
            phantom: PhantomData,
        }
    }

    pub fn memo(&mut self, memo: impl Into<String>) -> &mut Self {
        self.memo = Some(memo.into());
        self
    }

    pub fn operator(&mut self, id: AccountId) -> &mut Self {
        self.id = Some(TransactionId::new(id));
        self
    }

    pub fn node(&mut self, id: AccountId) -> &mut Self {
        self.node = Some(id);
        self
    }

    pub fn sign(&mut self, secret: SecretKey) -> &mut Self {
        self.secrets.push(secret);
        self
    }

    pub fn execute(&mut self) -> Result<TransactionResponse, Error> {
        use self::proto::Transaction::TransactionBody_oneof_data::*;

        let id = self
            .id
            .as_ref()
            .ok_or_else(|| ErrorKind::MissingField("operator"))?
            .clone();

        let mut tx: proto::Transaction::Transaction = self.to_proto()?;
        let client =
            proto::CryptoService_grpc::CryptoServiceClient::with_client(Arc::clone(&self.client));

        let o = grpc::RequestOptions::default();

        let response = match tx.mut_body().data {
            Some(cryptoCreateAccount(_)) => client.create_account(o, tx),
            Some(cryptoTransfer(_)) => client.crypto_transfer(o, tx),
            Some(cryptoDeleteClaim(_)) => client.delete_claim(o, tx),
            Some(cryptoDelete(ref mut data)) => {
                if !data.has_transferAccountID() {
                    // default the transfer account ID to the operator of the transaction
                    data.set_transferAccountID(id.account_id.to_proto()?);
                }

                client.crypto_delete(o, tx)
            }

            _ => unimplemented!(),
        };

        // TODO: Implement async
        let response = response.wait_drop_metadata()?;

        match response.get_nodeTransactionPrecheckCode().into() {
            PreCheckCode::Ok => Ok(TransactionResponse { id }),
            code => Err(ErrorKind::PreCheck(code))?,
        }
    }

    /// Return a mutable reference to the underlying type of the inner builder
    #[inline]
    pub(crate) fn inner(&mut self) -> &mut T {
        match self
            .inner
            .query_mut::<Any>()
            .and_then(|inner| inner.downcast_mut())
        {
            Some(inner) => inner,

            // Not possible in safe rust to get here
            _ => unreachable!(),
        }
    }
}

impl<T> ToProto<proto::Transaction::Transaction> for Transaction<T> {
    fn to_proto(&self) -> Result<proto::Transaction::Transaction, Error> {
        let body = ToProto::<proto::Transaction::TransactionBody>::to_proto(self)?;

        // NOTE: This cannot fail.
        let body_bytes = body.write_to_bytes().unwrap();

        let signatures: Result<Vec<proto::BasicTypes::Signature>, Error> = self
            .secrets
            .iter()
            .map(|secret| Ok(secret.sign(&body_bytes).to_proto()?))
            .collect();

        let mut signature_list = proto::BasicTypes::SignatureList::new();
        signature_list.set_sigs(RepeatedField::from_vec(signatures?));

        let mut tx = proto::Transaction::Transaction::new();
        tx.set_body(body);
        tx.set_sigs(signature_list);

        Ok(tx)
    }
}

impl<T> ToProto<proto::Transaction::TransactionBody> for Transaction<T> {
    fn to_proto(&self) -> Result<proto::Transaction::TransactionBody, Error> {
        // Get a reference to the trait implementation for ToProto for the inner builder
        let inner: &dyn ToProto<proto::Transaction::TransactionBody_oneof_data> =
            match self.inner.query_ref() {
                Some(inner) => inner,

                // Not possible in safe rust to get here
                _ => unreachable!(),
            };

        let tx_id = self
            .id
            .as_ref()
            .ok_or_else(|| ErrorKind::MissingField("operator"))?;

        let mut body = proto::Transaction::TransactionBody::new();
        let node = self.node.ok_or_else(|| ErrorKind::MissingField("node"))?;

        body.set_nodeAccountID(node.to_proto()?);
        body.set_transactionValidDuration(Duration::from_secs(120).to_proto()?);
        // TODO: Figure out a good way to do fees
        body.set_transactionFee(10);
        body.set_generateRecord(false);
        body.set_transactionID(tx_id.to_proto()?);
        body.data = Some(inner.to_proto()?);
        body.set_memo(if let Some(memo) = &self.memo {
            memo.to_owned()
        } else {
            String::new()
        });

        Ok(body)
    }
}
