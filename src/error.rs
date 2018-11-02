use crate::PreCheckCode;
use failure_derive::Fail;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "missing required field: `{}`", _0)]
    MissingField(&'static str),

    #[fail(display = "expected string of the format: {:?}", _0)]
    Parse(&'static str),

    #[fail(display = "transaction failed the pre-check: {:?}", _0)]
    PreCheck(PreCheckCode),
}
