use cdrs_tokio::load_balancing::SingleNode;
use cdrs_tokio::{
    authenticators::NoneAuthenticator,
    cluster::{ClusterTcpConfig, NodeTcpConfigBuilder, TcpConnectionPool, TcpConnectionsManager},
};
use cdrs_tokio::{cluster::session, cluster::session::Session, load_balancing::RoundRobin};

pub const CREATE_KEYSPACE: &str = "CREATE KEYSPACE IF NOT EXISTS amazon 
    WITH REPLICATION = {
            'class' : 'SimpleStrategy', 
            'replication_factor' : 1 
    };";

pub const CREATE_TABLE: &str = "CREATE TABLE IF NOT EXISTS amazon.prices ( 
    url text, 
    name text,
    time timestamp, 
    price decimal, 
    PRIMARY KEY(url, time) 
);";

pub async fn new_session(
    addr: &str,
) -> Result<Session<SingleNode<TcpConnectionPool<NoneAuthenticator>>>, anyhow::Error> {
    let node = NodeTcpConfigBuilder::new(addr, NoneAuthenticator {}).build();
    let cluster_config = ClusterTcpConfig(vec![node]);
    Ok(session::new(&cluster_config, SingleNode::new()).await?)
}
