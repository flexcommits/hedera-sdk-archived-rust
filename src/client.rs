use std::{sync::Arc, time::Duration};

use failure::{format_err, Error};
use itertools::Itertools;

use crate::{
    query::{
        Query, QueryGetAccountBalance, QueryGetAccountBalanceResponse, QueryGetTransactionReceipt,
        QueryGetTransactionReceiptResponse,
    },
    transaction::{
        Transaction, TransactionCryptoCreate, TransactionCryptoDelete,
        TransactionCryptoDeleteClaim, TransactionCryptoUpdate,
    },
    AccountId, TransactionId,
};

pub struct Client {
    pub(crate) inner: Arc<grpc::Client>,
}

impl Client {
    pub fn new(address: impl AsRef<str>) -> Result<Self, Error> {
        let address = address.as_ref();
        let (host, port) = address.split(':').next_tuple().ok_or_else(|| {
            format_err!("failed to parse 'host:port' from address: {:?}", address)
        })?;

        let port = port.parse()?;

        let inner = Arc::new(grpc::Client::new_plain(
            &host,
            port,
            grpc::ClientConf {
                http: httpbis::ClientConf {
                    no_delay: Some(true),
                    connection_timeout: Some(Duration::from_secs(5)),
                    ..httpbis::ClientConf::default()
                },
            },
        )?);

        Ok(Self { inner })
    }

    /// Create a new account. After the account is created, the AccountID for it is in the
    /// receipt, or can be retrieved with a GetByKey query, or by asking for a Record of the
    /// transaction to be created, and retrieving that.
    #[inline]
    pub fn create_account(&self) -> Transaction<TransactionCryptoCreate> {
        TransactionCryptoCreate::new(self)
    }

    #[inline]
    pub fn account(&self, id: AccountId) -> PartialAccountMessage {
        PartialAccountMessage(self, id)
    }

    #[inline]
    pub fn transaction(&self, id: TransactionId) -> PartialTransactionMessage {
        PartialTransactionMessage(self, id)
    }
}

pub struct PartialAccountMessage<'a>(&'a Client, AccountId);

impl<'a> PartialAccountMessage<'a> {
    /// Get the balance of a crypto-currency account.
    #[inline]
    pub fn balance(self) -> Query<QueryGetAccountBalanceResponse> {
        QueryGetAccountBalance::new(self.0, self.1)
    }

    /// Change properties for the given account. Any missing field is ignored (left unchanged).
    /// This transaction must be signed by the existing key for this account.
    #[inline]
    pub fn update(self) -> Transaction<TransactionCryptoUpdate> {
        TransactionCryptoUpdate::new(self.0, self.1)
    }

    /// Mark an account as deleted, moving all its current hbars to another account.
    /// It will remain in the ledger, marked as deleted, until it expires.
    #[inline]
    pub fn delete(self) -> Transaction<TransactionCryptoDelete> {
        TransactionCryptoDelete::new(self.0, self.1)
    }

    #[inline]
    pub fn claim(self, hash: impl Into<Vec<u8>>) -> PartialAccountClaimMessage<'a> {
        PartialAccountClaimMessage(self, hash.into())
    }
}

pub struct PartialAccountClaimMessage<'a>(PartialAccountMessage<'a>, Vec<u8>);

impl<'a> PartialAccountClaimMessage<'a> {
    /// Delete a claim hash that was attached to the given account.
    /// This transaction is valid if signed by all the keys used for transfers out of the account.
    #[inline]
    pub fn delete(self) -> Transaction<TransactionCryptoDeleteClaim> {
        TransactionCryptoDeleteClaim::new((self.0).0, (self.0).1, self.1)
    }
}

pub struct PartialTransactionMessage<'a>(&'a Client, TransactionId);

impl<'a> PartialTransactionMessage<'a> {
    /// Get the receipt of a transaction, given its transaction ID.
    ///
    /// Once a transaction reaches consensus, then information about whether it succeeded or
    /// failed will be available until the end of the receipt period.
    #[inline]
    pub fn receipt(self) -> Query<QueryGetTransactionReceiptResponse> {
        QueryGetTransactionReceipt::new(self.0, self.1)
    }
}
