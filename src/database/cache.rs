use color_eyre::Result;
use sled::IVec;

use crate::database::graph::GraphDb;
use crate::matrix::MatrixVersionServer;
use tracing::{error, info};

#[derive(Debug)]
pub struct CacheDb {
    db: sled::Db,
    graph: GraphDb,
}

impl CacheDb {
    pub fn new() -> Self {
        CacheDb::default()
    }

    pub fn set_server_address(&self, server_name: &str, server_address: String) -> Result<()> {
        self.db.insert(
            format!("address/{}", server_name).as_bytes(),
            server_address.as_bytes(),
        )?;
        self.db.flush()?;
        Ok(())
    }

    pub fn set_server_version(
        &self,
        server_name: &str,
        server_version: MatrixVersionServer,
    ) -> Result<()> {
        let server_version_bytes = bincode::serialize(&server_version)?;
        self.db.insert(
            format!("version/{}", server_name).as_bytes(),
            server_version_bytes,
        )?;
        self.db.flush()?;
        Ok(())
    }

    pub fn contains_server(&self, server_name: &str) -> bool {
        if let Ok(res) = self
            .db
            .contains_key(format!("address/{}", server_name).as_bytes())
        {
            return res;
        }
        false
    }

    pub fn get_server_address(&self, server_name: &str) -> Option<IVec> {
        if let Ok(res) = self.db.get(format!("address/{}", server_name).as_bytes()) {
            return res;
        } else {
            error!("Failed to get Server from sled");
        }
        None
    }

    pub fn get_server_version(&self, server_name: &str) -> Result<Option<MatrixVersionServer>> {
        match self.db.get(format!("version/{}", server_name).as_bytes()) {
            Ok(res) => {
                if let Some(bytes) = res {
                    let server_version: MatrixVersionServer = bincode::deserialize(bytes.as_ref())?;
                    return Ok(Some(server_version));
                }
            }
            Err(e) => error!("Failed to get version: {}", e),
        }

        Ok(None)
    }

    pub fn get_all_addresses(
        &self,
    ) -> impl DoubleEndedIterator<Item = sled::Result<IVec>> + Send + Sync {
        let prefix: &[u8] = b"address/";
        let r = self.db.scan_prefix(prefix);
        r.values()
    }

    pub fn get_all_servers(
        &self,
    ) -> impl DoubleEndedIterator<Item = sled::Result<IVec>> + Send + Sync {
        let prefix: &[u8] = b"address/";
        let r = self.db.scan_prefix(prefix);
        r.keys()
    }
}

impl Default for CacheDb {
    fn default() -> Self {
        info!("Created new db");
        let db = sled::Config::default()
            .path("./storage/cache".to_owned())
            .use_compression(true)
            .open()
            .unwrap();
        let tree = db.open_tree(b"graph").unwrap();
        let graph = GraphDb::new(tree);
        CacheDb { db, graph }
    }
}
