pub mod collect;
pub mod graph;
pub mod options;
pub mod order;
pub mod overlay;
pub mod workflow;

use collect::Collection;

pub struct Resolved {
    pub shipped: overlay::Index,
    pub collected: Collection,
}
