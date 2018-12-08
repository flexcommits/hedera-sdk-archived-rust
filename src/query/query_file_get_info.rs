use crate::{
    proto::{self, Query::Query_oneof_query, QueryHeader::QueryHeader, ToProto},
    query::{Query, QueryInner},
    Client, FileId, FileInfo,
};
use failure::Error;
use try_from::TryInto;

pub struct QueryFileGetInfo {
    file: FileId,
}

impl QueryFileGetInfo {
    pub fn new(client: &Client, file: FileId) -> Query<FileInfo> {
        Query::new(client, Self { file })
    }
}

impl QueryInner for QueryFileGetInfo {
    type Response = FileInfo;

    fn get(&self, mut response: proto::Response::Response) -> Result<Self::Response, Error> {
        response.take_fileGetInfo().take_fileInfo().try_into()
    }

    fn to_query_proto(&self, header: QueryHeader) -> Result<Query_oneof_query, Error> {
        let mut query = proto::FileGetInfo::FileGetInfoQuery::new();
        query.set_header(header);
        query.set_fileID(self.file.to_proto()?);

        Ok(Query_oneof_query::fileGetInfo(query))
    }
}
