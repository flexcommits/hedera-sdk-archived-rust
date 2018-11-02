use crate::{
    error::ErrorKind,
    proto::{self, ToProto},
    timestamp::Timestamp,
    AccountId,
};
use failure::Error;
use itertools::Itertools;
use std::{fmt, str::FromStr};

#[derive(Debug, PartialEq)]
#[repr(C)]
pub struct TransactionId {
    pub account_id: AccountId,
    pub transaction_valid_start: Timestamp,
}

impl TransactionId {
    pub fn new(account_id: AccountId) -> Self {
        TransactionId {
            account_id,
            // Allows the transaction to be accepted as long as the
            // server is not more than 5 seconds behind us
            transaction_valid_start: Timestamp::now() - 5,
        }
    }
}

impl fmt::Display for TransactionId {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{}@{}", self.account_id, self.transaction_valid_start)
    }
}

impl FromStr for TransactionId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (account_id, timestamp) = s
            .split('@')
            .next_tuple()
            .ok_or_else(|| ErrorKind::Parse("{realm}:{shard}:{account}@{seconds}.{nanos}"))?;

        Ok(Self {
            account_id: account_id.parse()?,
            transaction_valid_start: timestamp.parse()?,
        })
    }
}

impl From<crate::proto::BasicTypes::TransactionID> for TransactionId {
    fn from(mut pb: crate::proto::BasicTypes::TransactionID) -> Self {
        Self {
            account_id: pb.take_accountID().into(),
            transaction_valid_start: pb.take_transactionValidStart().into(),
        }
    }
}

impl ToProto<proto::BasicTypes::TransactionID> for TransactionId {
    fn to_proto(&self) -> Result<proto::BasicTypes::TransactionID, Error> {
        let mut id = proto::BasicTypes::TransactionID::new();
        id.set_transactionValidStart(self.transaction_valid_start.to_proto()?);
        id.set_accountID(self.account_id.to_proto()?);

        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::TransactionId;
    use crate::{AccountId, Timestamp};
    use failure::Error;

    #[test]
    fn test_display() {
        let account_id = AccountId::new(7, 5, 1001);
        let transaction_valid_start = Timestamp {
            seconds: 1234567,
            nanos: 10001,
        };
        let transaction_id = TransactionId {
            account_id,
            transaction_valid_start,
        };

        assert_eq!(format!("{}", transaction_id), "7:5:1001@1234567.10001");
    }

    #[test]
    fn test_parse() -> Result<(), Error> {
        let account_id = AccountId::new(7, 5, 1001);
        let transaction_valid_start = Timestamp {
            seconds: 1234567,
            nanos: 10001,
        };
        let transaction_id = TransactionId {
            account_id,
            transaction_valid_start,
        };

        assert_eq!(
            "7:5:1001@1234567.10001".parse::<TransactionId>()?,
            transaction_id
        );

        Ok(())
    }
}
