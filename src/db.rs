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

pub const CREATE_PRODUCT_TABLE: &str = "CREATE TABLE IF NOT EXISTS amazon.prices ( 
    name text,
    url text, 
    time timestamp, 
    price decimal, 
    PRIMARY KEY(name, time) 
);";

pub const ADD_PRODUCT_PRICE: &str =
    "INSERT INTO amazon.prices (name, url, time, price) VALUES (?, ?, ?, ?)";

pub async fn new_session(
    addr: &str,
) -> Result<Session<SingleNode<TcpConnectionPool<NoneAuthenticator>>>, anyhow::Error> {
    let node = NodeTcpConfigBuilder::new(addr, NoneAuthenticator {}).build();
    let cluster_config = ClusterTcpConfig(vec![node]);
    Ok(session::new(&cluster_config, SingleNode::new()).await?)
}
