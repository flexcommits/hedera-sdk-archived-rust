use crate::{
    crypto::SecretKey,
    error::ErrorKind,
    proto::{
        self,
        CryptoService_grpc::{CryptoService, CryptoServiceClient},
        FileService_grpc::{FileService, FileServiceClient},
        SmartContractService_grpc::{SmartContractService, SmartContractServiceClient},
        ToProto,
    },
    AccountId, Client, TransactionId,
};
use failure::Error;
use protobuf::Message;
use query_interface::Object;
use std::{any::Any, marker::PhantomData, mem::swap, sync::Arc, time::Duration};

pub struct TransactionBuilder<T> {
    id: Option<TransactionId>,
    node: Option<AccountId>,
    memo: Option<String>,
    generate_record: bool,
    fee: u64,
    pub(crate) inner: Box<dyn Object>,
    phantom: PhantomData<T>,
}

pub struct TransactionRaw {
    bytes: Vec<u8>,
    pub(crate) tx: proto::Transaction::Transaction,
}

enum TransactionKind<T> {
    Empty,
    Err(Error),
    Builder(TransactionBuilder<T>),
    Raw(TransactionRaw),
}

impl<T> TransactionKind<T> {
    fn take(&mut self) -> Self {
        let mut temp = TransactionKind::Empty;
        swap(&mut temp, self);
        temp
    }
}

pub struct Transaction<T, S = TransactionBuilder<T>> {
    crypto_service: Arc<CryptoServiceClient>,
    file_service: Arc<FileServiceClient>,
    contract_service: Arc<SmartContractServiceClient>,
    secret: Option<Arc<SecretKey>>,
    kind: TransactionKind<T>,
    phantom: PhantomData<S>,
}

impl<T: 'static> Transaction<T, TransactionBuilder<T>> {
    pub(crate) fn new(client: &Client, inner: T) -> Self
    where
        T: Object + ToProto<proto::Transaction::TransactionBody_oneof_data> + 'static,
    {
        Self {
            crypto_service: client.crypto.clone(),
            file_service: client.file.clone(),
            contract_service: client.contract.clone(),
            secret: client.operator_secret.clone(),
            kind: TransactionKind::Builder(TransactionBuilder {
                id: client.operator.map(TransactionId::new),
                node: client.node.clone(),
                memo: None,
                inner: Box::<T>::new(inner) as Box<dyn Object>,
                fee: 10,
                generate_record: false,
                phantom: PhantomData,
            }),
            phantom: PhantomData,
        }
    }

    pub fn memo(&mut self, memo: impl Into<String>) -> &mut Self {
        if let Some(state) = self.as_builder() {
            state.memo = Some(memo.into());
        }

        self
    }

    pub fn operator(&mut self, id: AccountId, secret: SecretKey) -> &mut Self {
        if let Some(state) = self.as_builder() {
            state.id = Some(TransactionId::new(id));
        }
        self.secret = Some(Arc::new(secret));

        self
    }

    pub fn node(&mut self, id: AccountId) -> &mut Self {
        if let Some(state) = self.as_builder() {
            state.node = Some(id);
        }

        self
    }

    /// The fee the client pays, which is split between the network and the node.
    pub fn fee(&mut self, fee: u64) -> &mut Self {
        if let Some(state) = self.as_builder() {
            state.fee = fee;
        }

        self
    }

    /// Should a record of this transaction be generated?
    /// A receipt is always generated, but the record is optional.
    pub fn generate_record(&mut self, generate: bool) -> &mut Self {
        if let Some(state) = self.as_builder() {
            state.generate_record = generate;
        }

        self
    }

    pub fn sign(&mut self, secret: &SecretKey) -> &mut Transaction<T, TransactionRaw> {
        self.build().sign(secret)
    }

    pub fn execute(&mut self) -> Result<TransactionId, Error> {
        self.build().execute()
    }

    #[inline]
    fn as_builder(&mut self) -> Option<&mut TransactionBuilder<T>> {
        match &mut self.kind {
            TransactionKind::Builder(ref mut state) => Some(state),

            TransactionKind::Raw(_) => {
                // should never be able to happen (in Rust)
                panic!("cannot edit a transaction after it has been signed")
            }

            TransactionKind::Err(_) => {
                // should never be able to happen (in Rust)
                None
            }

            _ => {
                // should never be able to happen (in Rust)
                panic!("transaction already executed")
            }
        }
    }

    #[inline]
    pub(crate) fn inner(&mut self) -> &mut T {
        // not possible to fail in safe rust
        match self
            .as_builder()
            .unwrap()
            .inner
            .query_mut::<dyn Any>()
            .and_then(|inner| inner.downcast_mut())
        {
            Some(inner) => inner,

            // not possible in safe rust to get here
            _ => unreachable!(),
        }
    }
}

impl<T> Transaction<T, TransactionRaw> {
    #[inline]
    pub(crate) fn as_raw(&mut self) -> Option<&mut TransactionRaw> {
        match &mut self.kind {
            TransactionKind::Builder(_) => {
                // not possible in safe rust
                unreachable!()
            }

            TransactionKind::Raw(ref mut state) => Some(state),

            TransactionKind::Err(_) => None,

            TransactionKind::Empty => {
                // should never be able to happen (in Rust)
                panic!("transaction already executed")
            }
        }
    }

    pub fn sign(&mut self, secret: &SecretKey) -> &mut Self {
        use self::proto::{
            BasicTypes::HederaFunctionality::*, Transaction::TransactionBody_oneof_data::*,
        };

        if let Some(state) = self.as_raw() {
            // note: this cannot fail
            let mut signature = secret.sign(&state.bytes).to_proto().unwrap();

            // determine what kind of tx we have
            let kind = match state.tx.body.as_ref().unwrap().data {
                Some(fileCreate(_)) => Some(FileCreate),
                Some(fileAppend(_)) => Some(FileAppend),
                _ => None,
            };

            if !state.tx.has_sigs() {
                state.tx.set_sigs(proto::BasicTypes::SignatureList::new());
            }

            // note: this cannot fail
            let signatures = &mut state.tx.sigs.as_mut().unwrap().sigs;

            // signature #0 is for operator
            // signature #1 is for:
            //  - owner of _thing_ being created
            //  - # correspond to transfer

            if kind == Some(FileCreate) || kind == Some(FileAppend) {
                // IF we are on signature #1 and we operating on a file or contract,
                // place the signature into a signature list

                let mut sig = proto::BasicTypes::Signature::new();
                sig.signature = signature.signature;

                let mut sigs = proto::BasicTypes::SignatureList::new();
                sigs.sigs.push(sig);

                signature = proto::BasicTypes::Signature::new();
                signature.set_signatureList(sigs);
            }

            signatures.push(signature);
        }

        self
    }

    pub fn execute(&mut self) -> Result<TransactionId, Error> {
        use self::proto::Transaction::TransactionBody_oneof_data::*;

        let state = match self.kind.take() {
            TransactionKind::Raw(state) => state,
            TransactionKind::Builder(_) => unreachable!(),
            TransactionKind::Empty => panic!("transaction already executed"),
            TransactionKind::Err(error) => return Err(error),
        };

        let mut tx = state.tx;
        log::trace!(target: "hedera::transaction", "sent: {:#?}", tx);

        let o = grpc::RequestOptions::default();

        // sign as the operator

        if let Some(secret) = &self.secret {
            let signature = secret.sign(&state.bytes).to_proto().unwrap();
            let signatures = &mut tx.sigs.as_mut().unwrap().sigs;

            signatures.insert(0, signature);
        }

        // note: cannot fail
        let id = tx
            .body
            .as_ref()
            .unwrap()
            .transactionID
            .as_ref()
            .unwrap()
            .clone();

        let operator = id.accountID.as_ref().unwrap().clone();

        let response = match tx.mut_body().data {
            Some(cryptoCreateAccount(_)) => self.crypto_service.create_account(o, tx),

            Some(cryptoTransfer(_)) => self.crypto_service.crypto_transfer(o, tx),

            Some(cryptoDeleteClaim(_)) => self.crypto_service.delete_claim(o, tx),

            Some(cryptoDelete(ref mut data)) => {
                if !data.has_transferAccountID() {
                    // default the transfer account ID to the operator of the transaction
                    data.set_transferAccountID(operator);
                }

                self.crypto_service.crypto_delete(o, tx)
            }

            Some(fileCreate(_)) => self.file_service.create_file(o, tx),
            Some(fileAppend(_)) => self.file_service.append_content(o, tx),

            Some(contractCreateInstance(_)) => self.contract_service.create_contract(o, tx),

            _ => unimplemented!(),
        };

        // TODO: Implement async
        let response = response.wait_drop_metadata()?;
        log::trace!("recv: {:#?}", response);

        try_precheck!(response).map(|_| id.into())
    }
}

impl<T: 'static, S: 'static> Transaction<T, S> {
    #[inline]
    pub(crate) fn take_raw(&mut self) -> Result<TransactionRaw, Error> {
        match self.kind.take() {
            TransactionKind::Builder(_) => self.build().take_raw(),

            TransactionKind::Raw(state) => Ok(state),

            TransactionKind::Err(err) => Err(err),

            TransactionKind::Empty => {
                panic!("transaction already executed")
            }
        }
    }

    // Transition from builder to raw
    // Done before the first signature or execute
    #[inline]
    pub(crate) fn build(&mut self) -> &mut Transaction<T, TransactionRaw> {
        match &self.kind {
            TransactionKind::Empty => panic!("transaction already executed"),

            TransactionKind::Raw(_) | TransactionKind::Err(_) => {
                // Do nothing; we are already built
                // this is 100% safe; its changing a marker type parameter
                return unsafe { std::mem::transmute(self) };
            }

            _ => {
                // Fall-through to do something fun
            }
        }

        if let TransactionKind::Builder(state) = self.kind.take() {
            match state.to_proto() {
                Ok(tx) => {
                    // note: this cannot fail
                    let tx: proto::Transaction::Transaction = tx;
                    let bytes = tx.body.as_ref().unwrap().write_to_bytes().unwrap();

                    self.kind = TransactionKind::Raw(TransactionRaw { tx, bytes })
                }

                Err(error) => {
                    self.kind = TransactionKind::Err(error);
                }
            }
        }

        // this is 100% safe; its changing a marker type parameter
        unsafe { std::mem::transmute(self) }
    }
}

impl<T> ToProto<proto::Transaction::Transaction> for TransactionBuilder<T> {
    fn to_proto(&self) -> Result<proto::Transaction::Transaction, Error> {
        let mut tx = proto::Transaction::Transaction::new();
        tx.set_body(self.to_proto()?);

        Ok(tx)
    }
}

impl<T> ToProto<proto::Transaction::TransactionBody> for TransactionBuilder<T> {
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
        body.set_transactionFee(self.fee);
        body.set_generateRecord(self.generate_record);
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
