use crate::{
    claim::Claim,
    crypto::PublicKey,
    id::AccountId,
    proto::{self, ToProto, Transaction::Transaction_oneof_bodyData},
    transaction::Transaction,
    Client,
};
use failure::Error;
use query_interface::{interfaces, vtable_for};
use std::any::Any;

#[derive(Debug)]
pub struct TransactionCryptoAddClaim {
    account: AccountId,
    hash: Vec<u8>,
    keys: Vec<PublicKey>,
}

interfaces!(
    TransactionCryptoAddClaim: dyn Any,
    dyn ToProto<Transaction_oneof_bodyData>
);

impl TransactionCryptoAddClaim {
    pub fn new(client: &Client, account: AccountId, hash: Vec<u8>) -> Transaction<Self> {
        Transaction::new(
            client,
            Self {
                account,
                hash,
                keys: Vec::new(),
            },
        )
    }
}

impl Transaction<TransactionCryptoAddClaim> {
    #[inline]
    pub fn key(&mut self, key: PublicKey) -> &mut Self {
        self.inner().keys.push(key);
        self
    }
}

impl ToProto<Transaction_oneof_bodyData> for TransactionCryptoAddClaim {
    fn to_proto(&self) -> Result<Transaction_oneof_bodyData, Error> {
        let mut data = proto::CryptoAddClaim::CryptoAddClaimTransactionBody::new();
        data.set_accountID(self.account.to_proto()?);

        let claim = Claim {
            account: self.account,
            hash: self.hash.clone(),
            keys: self.keys.clone(),
        };

        data.set_claim(claim.to_proto()?);

        Ok(Transaction_oneof_bodyData::cryptoAddClaim(data))
    }
}
