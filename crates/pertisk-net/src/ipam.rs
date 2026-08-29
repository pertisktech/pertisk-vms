use crate::{NetError, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ipv4Net {
    pub network: u32,
    pub prefix: u8,
}

impl Ipv4Net {
    pub fn parse(cidr: &str) -> Result<Self> {
        parse_cidr(cidr)
    }

    pub fn mask(self) -> u32 {
        if self.prefix == 0 {
            0
        } else {
            !0u32 << (32 - self.prefix)
        }
    }

    pub fn contains(self, ip: u32) -> bool {
        (ip & self.mask()) == self.network
    }

    pub fn overlaps(self, other: Self) -> bool {
        self.contains(other.network) || other.contains(self.network)
    }

    pub fn nth(self, n: u32) -> u32 {
        self.network.saturating_add(n)
    }

    pub fn broadcast(self) -> u32 {
        self.network | !self.mask()
    }

    pub fn to_cidr_string(self) -> String {
        format!("{}/{}", ipv4_string(self.network), self.prefix)
    }

    pub fn allocate(self, gateway: Option<&str>, used: &[String]) -> Result<u32> {
        let gw = gateway.map(parse_ipv4).transpose()?;
        let start = self.nth(2);
        let end = self.broadcast().saturating_sub(1);
        if start > end {
            return Err(NetError::PoolExhausted(self.to_cidr_string()));
        }
        let used: Vec<u32> = used.iter().filter_map(|s| parse_ipv4(s).ok()).collect();
        let mut ip = start;
        while ip <= end {
            if Some(ip) != gw && !used.contains(&ip) {
                return Ok(ip);
            }
            ip += 1;
        }
        Err(NetError::PoolExhausted(self.to_cidr_string()))
    }
}

pub fn parse_cidr(s: &str) -> Result<Ipv4Net> {
    let (addr, prefix) = s
        .split_once('/')
        .ok_or_else(|| NetError::Invalid(format!("cidr must look like 10.88.0.0/24, got {s}")))?;
    let prefix: u8 = prefix
        .parse()
        .map_err(|_| NetError::Invalid(format!("invalid prefix in {s}")))?;
    if prefix > 32 {
        return Err(NetError::Invalid("prefix must be 0..=32".into()));
    }
    let ip = parse_ipv4(addr)?;
    let net = Ipv4Net { network: 0, prefix };
    let network = ip & net.mask();
    Ok(Ipv4Net { network, prefix })
}

pub fn parse_ipv4(s: &str) -> Result<u32> {
    let mut parts = s.split('.');
    let mut acc = 0u32;
    for _ in 0..4 {
        let p = parts
            .next()
            .ok_or_else(|| NetError::Invalid(format!("invalid ipv4 {s}")))?;
        let n: u8 = p
            .parse()
            .map_err(|_| NetError::Invalid(format!("invalid ipv4 {s}")))?;
        acc = (acc << 8) | u32::from(n);
    }
    if parts.next().is_some() {
        return Err(NetError::Invalid(format!("invalid ipv4 {s}")));
    }
    Ok(acc)
}

pub fn ipv4_string(ip: u32) -> String {
    format!(
        "{}.{}.{}.{}",
        (ip >> 24) & 0xff,
        (ip >> 16) & 0xff,
        (ip >> 8) & 0xff,
        ip & 0xff
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_allocates() {
        let net = Ipv4Net::parse("10.88.0.0/24").unwrap();
        assert_eq!(ipv4_string(net.nth(1)), "10.88.0.1");
        let ip = net.allocate(Some("10.88.0.1"), &[]).unwrap();
        assert_eq!(ipv4_string(ip), "10.88.0.2");
        let ip = net
            .allocate(Some("10.88.0.1"), &["10.88.0.2".into()])
            .unwrap();
        assert_eq!(ipv4_string(ip), "10.88.0.3");
    }

    #[test]
    fn detects_overlapping_networks() {
        let lan = Ipv4Net::parse("10.1.1.0/24").unwrap();
        assert!(lan.overlaps(Ipv4Net::parse("10.1.1.0/25").unwrap()));
        assert!(lan.overlaps(Ipv4Net::parse("10.1.0.0/16").unwrap()));
        assert!(!lan.overlaps(Ipv4Net::parse("10.88.0.0/24").unwrap()));
    }
}
