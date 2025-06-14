use async_graphql::Object;
use ipnet::IpNet;

pub struct IpNetGraphql(IpNet);
impl From<IpNet> for IpNetGraphql {
    fn from(net: IpNet) -> Self {
        Self(net)
    }
}
impl From<IpNetGraphql> for IpNet {
    fn from(net: IpNetGraphql) -> Self {
        net.0
    }
}

#[Object]
impl IpNetGraphql {
    async fn ip(&self) -> String {
        self.0.addr().to_string()
    }
    async fn net(&self) -> String {
        self.0.network().to_string()
    }
    async fn mask(&self) -> u8 {
        self.0.prefix_len()
    }
    async fn display(&self) -> String {
        self.0.to_string()
    }
}
