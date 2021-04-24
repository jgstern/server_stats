#[derive(Debug)]
pub struct GraphDb {
    tree: sled::Tree,
}

impl GraphDb {
    pub fn new(tree: sled::Tree) -> Self {
        GraphDb { tree }
    }
}
