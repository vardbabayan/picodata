use crate::failure_domain::FailureDomain;
use crate::instance::{Instance, InstanceId};
use crate::replicaset::ReplicasetId;
use crate::storage::ToEntryIter as _;
use crate::traft::{error::Error, node, Address, PeerAddress, Result};

#[derive(Clone, Debug, ::serde::Serialize, ::serde::Deserialize)]
pub struct OkResponse {
    pub instance: Box<Instance>,
    pub peer_addresses: Vec<PeerAddress>,
    pub box_replication: Vec<Address>,
    // Other parameters necessary for box.cfg()
    // TODO
}

crate::define_rpc_request! {
    fn proc_raft_join(req: Request) -> Result<Response> {
        let node = node::global()?;

        let cluster_id = node
            .raft_storage
            .cluster_id()?
            .expect("cluster_id is set on boot");

        if req.cluster_id != cluster_id {
            return Err(Error::ClusterIdMismatch {
                instance_cluster_id: req.cluster_id,
                cluster_cluster_id: cluster_id,
            });
        }

        match node.handle_join_request_and_wait(req) {
            Ok((instance, replication_addresses)) => {
                // A joined instance needs to communicate with other nodes.
                // TODO: limit the number of entries sent to reduce response size.
                let peer_addresses = node.storage.peer_addresses.iter()?.collect();

                Ok(Response::Ok(OkResponse {
                    instance,
                    peer_addresses,
                    box_replication: replication_addresses.into_iter().collect(),
                }))

            },
            Err(Error::NotALeader) => {
                let leader_id = node.status().leader_id;
                let leader_address = leader_id.and_then(|id| node.storage.peer_addresses.try_get(id).ok());
                let leader = match (leader_id, leader_address) {
                    (Some(raft_id), Some(address)) => Some(PeerAddress{raft_id, address}),
                    (_, _) => None
                };
                Ok(Response::ErrNotALeader(leader))
            }
            Err(e) => Err(e),
        }
    }

    /// Request to join the cluster.
    pub struct Request {
        pub cluster_id: String,
        pub instance_id: Option<InstanceId>,
        pub replicaset_id: Option<ReplicasetId>,
        pub advertise_address: String,
        pub failure_domain: FailureDomain,
    }

    /// Response to a [`join::Request`](Request).
    pub enum Response {
        Ok(OkResponse),
        ErrNotALeader(Option<PeerAddress>),
    }
}