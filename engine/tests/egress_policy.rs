//! Pure selected-peer policy regressions. No actual network, DNS, Android grant,
//! VPN/middlebox behavior or physical isolation is exercised.
use std::net::{IpAddr, Ipv6Addr, SocketAddr, SocketAddrV6};
use wasmdroid_engine::network::egress::{
    DenyReason, DestinationClass as C, EgressContext, EgressPolicy, NetworkPrefix,
    UnsupportedReason,
};
fn ip(value: &str) -> IpAddr {
    value.parse().unwrap()
}
fn prefix(value: &str) -> NetworkPrefix {
    NetworkPrefix::parse(value).unwrap()
}
fn policy(internet: bool, lan: bool) -> EgressPolicy {
    EgressPolicy::new(
        internet,
        lan,
        EgressContext::new(vec![], vec![], vec![]).unwrap(),
    )
}
fn contextual(own: &[&str], links: &[&str], nat64: &[&str]) -> EgressPolicy {
    EgressPolicy::new(
        true,
        true,
        EgressContext::new(
            own.iter().map(|s| ip(s)).collect(),
            links.iter().map(|s| prefix(s)).collect(),
            nat64.iter().map(|s| prefix(s)).collect(),
        )
        .unwrap(),
    )
}

#[test]
fn independent_native_grants_and_dns_do_not_treat_lan_as_internet() {
    for internet in [false, true] {
        for lan in [false, true] {
            let policy = policy(internet, lan);
            assert_eq!(policy.authorize_dns().is_ok(), internet || lan);
            for address in ["8.8.8.8", "2001:4860:4860::8888"] {
                assert_eq!(policy.authorize_ip(ip(address)).is_ok(), internet);
            }
            for address in [
                "10.0.0.1",
                "172.16.0.1",
                "192.168.1.1",
                "fd12:3456::1",
                "fc00::1",
            ] {
                assert_eq!(policy.authorize_ip(ip(address)).is_ok(), lan);
            }
        }
    }
}
#[test]
fn hard_denials_override_both_grants_and_mapped_alias_spellings() {
    let policy = policy(true, true);
    for address in [
        "0.0.0.0",
        "127.0.0.2",
        "::",
        "::1",
        "224.0.0.1",
        "ff02::1",
        "255.255.255.255",
        "10.203.0.0",
        "10.203.0.255",
        "::ffff:127.0.0.1",
        "::ffff:10.203.0.2",
    ] {
        assert!(
            matches!(policy.classify(ip(address)), C::Denied(_)),
            "{address}"
        );
        assert!(policy.authorize_ip(ip(address)).is_err());
    }
    for address in ["::ffff:8.8.8.8", "::ffff:192.168.1.2", "::8.8.8.8"] {
        assert_eq!(
            policy.classify(ip(address)),
            C::Unsupported(UnsupportedReason::Ipv4Alias)
        );
        assert!(
            policy
                .authorize_tcp(SocketAddr::new(ip(address), 443))
                .is_err()
        );
    }
}
#[test]
fn private_and_special_prefix_boundaries_are_not_complement_of_private() {
    let policy = policy(true, true);
    for address in [
        "9.255.255.255",
        "11.0.0.0",
        "172.15.255.255",
        "172.32.0.0",
        "192.167.255.255",
        "192.169.0.0",
        "100.63.255.255",
        "100.128.0.0",
        "198.17.255.255",
        "198.20.0.0",
    ] {
        assert_eq!(policy.classify(ip(address)), C::Internet, "{address}");
    }
    for address in [
        "10.0.0.0",
        "10.255.255.255",
        "172.16.0.0",
        "172.31.255.255",
        "192.168.0.0",
        "192.168.255.255",
    ] {
        assert_eq!(policy.classify(ip(address)), C::Lan, "{address}");
    }
    for address in [
        "0.1.2.3",
        "100.64.0.0",
        "100.127.255.255",
        "192.0.0.8",
        "192.0.0.170",
        "192.0.2.1",
        "198.18.0.0",
        "198.19.255.255",
        "198.51.100.1",
        "203.0.113.1",
        "240.0.0.1",
    ] {
        assert!(
            matches!(policy.classify(ip(address)), C::Denied(_)),
            "{address}"
        );
    }
}
#[test]
fn current_iana_longest_prefix_exceptions_do_not_inherit_parent_denial() {
    let policy = policy(true, false);
    for address in [
        "192.0.0.9",
        "192.0.0.10",
        "192.31.196.1",
        "192.52.193.1",
        "192.175.48.1",
        "2001:1::1",
        "2001:1::2",
        "2001:1::3",
        "2001:3::1",
        "2001:4:112::1",
        "2620:4f:8000::1",
    ] {
        assert_eq!(policy.classify(ip(address)), C::Internet, "{address}");
        assert!(policy.authorize_ip(ip(address)).is_ok());
    }
    assert!(matches!(policy.classify(ip("2001:1::4")), C::Denied(_)));
}
#[test]
fn ipv6_ordinary_scope_and_recent_documentation_are_explicit() {
    let policy = policy(true, true);
    assert_eq!(policy.classify(ip("2001:4860:4860::8888")), C::Internet);
    for address in [
        "100::1",
        "100:0:0:1::1",
        "2001:2::1",
        "2001:db8::1",
        "3fff::1",
        "3fff:fff::1",
        "5f00::1",
    ] {
        assert!(
            matches!(policy.classify(ip(address)), C::Denied(_)),
            "{address}"
        );
    }
    for address in [
        "169.254.1.2",
        "fe80::1",
        "fec0::1",
        "4000::1",
        "2002:808:808::1",
        "2001::1",
        "2001:20::1",
        "2001:30::1",
        "192.88.99.2",
    ] {
        assert!(
            matches!(policy.classify(ip(address)), C::Unsupported(_)),
            "{address}"
        );
    }
    assert_eq!(policy.classify(ip("3fff:1000::1")), C::Internet);
}
#[test]
fn own_host_addresses_and_real_subnet_broadcast_override_lan_and_internet() {
    let policy = contextual(
        &["8.8.8.8", "192.168.1.2", "fd12::1", "2001:4860::1"],
        &["192.168.1.0/24", "8.8.4.0/24"],
        &[],
    );
    for address in ["8.8.8.8", "192.168.1.2", "fd12::1", "2001:4860::1"] {
        assert_eq!(
            policy.classify(ip(address)),
            C::Denied(DenyReason::HostAddress)
        );
    }
    assert_eq!(
        policy.classify(ip("192.168.1.255")),
        C::Denied(DenyReason::Broadcast)
    );
    assert_eq!(
        policy.classify(ip("8.8.4.255")),
        C::Denied(DenyReason::Broadcast)
    );
    assert_eq!(policy.classify(ip("192.168.2.255")), C::Lan);
}
#[test]
fn known_on_link_global_is_unsupported_and_point_to_point_has_no_guessed_broadcast() {
    let policy = contextual(&[], &["8.8.8.0/24", "2001:4860::/64", "9.9.9.254/31"], &[]);
    for address in ["8.8.8.8", "2001:4860::abcd", "9.9.9.254", "9.9.9.255"] {
        assert_eq!(
            policy.classify(ip(address)),
            C::Unsupported(UnsupportedReason::OnLinkGlobal)
        );
        assert!(policy.authorize_ip(ip(address)).is_err());
    }
    assert_eq!(policy.classify(ip("8.8.9.8")), C::Internet);
}
#[test]
fn canonical_bounded_context_rejects_guessed_default_routes_and_ambiguous_translation() {
    for malformed in [
        "192.168.1.1/24",
        "2001:4860::1/64",
        "0.0.0.0/33",
        "::/129",
        "::/064",
        "::/+64",
        " fe80::/10",
        "fe80::%2/64",
    ] {
        assert!(NetworkPrefix::parse(malformed).is_err(), "{malformed}");
    }
    assert!(EgressContext::new(vec![ip("1.1.1.1"); 129], vec![], vec![]).is_err());
    assert!(EgressContext::new(vec![], vec![prefix("0.0.0.0/0")], vec![]).is_err());
    assert!(EgressContext::new(vec![], vec![], vec![prefix("2001:4860::/72")]).is_err());
    assert!(EgressContext::new(vec![], vec![], vec![prefix("10.0.0.0/32")]).is_err());
    assert!(
        EgressContext::new(
            vec![],
            vec![],
            vec![prefix("2001:4860::/32"), prefix("2001:4860:abcd::/48")]
        )
        .is_err()
    );
    assert!(EgressContext::new(vec![], vec![], vec![prefix("64:ff9b::/64")]).is_err());
}
#[test]
fn tcp_zero_port_and_zone_scope_do_not_acquire_destination_permission() {
    let policy = policy(true, true);
    assert!(
        policy
            .authorize_tcp(SocketAddr::new(ip("8.8.8.8"), 0))
            .is_err()
    );
    let peer = SocketAddrV6::new("2001:4860::1".parse::<Ipv6Addr>().unwrap(), 443, 0, 7);
    assert!(policy.authorize_tcp(peer.into()).is_err());
    assert!(policy.authorize_tcp("8.8.8.8:443".parse().unwrap()).is_ok());
}
#[test]
fn every_rfc6052_layout_decodes_the_same_public_ipv4_without_suffix_scanning() {
    for (network, address) in [
        ("2001:4860::/32", "2001:4860:808:404::"),
        ("2001:4860:ab00::/40", "2001:4860:ab08:804:4::"),
        ("2001:4860:abcd::/48", "2001:4860:abcd:808:4:400::"),
        ("2001:4860:abcd:ef00::/56", "2001:4860:abcd:ef08:8:404::"),
        (
            "2001:4860:abcd:ef01::/64",
            "2001:4860:abcd:ef01:8:804:400:0",
        ),
        (
            "2001:4860:abcd:ef01:0:1234::/96",
            "2001:4860:abcd:ef01:0:1234:808:404",
        ),
    ] {
        let policy = contextual(&[], &[], &[network]);
        assert_eq!(
            policy.classify(ip(address)),
            C::Internet,
            "{network} {address}"
        );
        assert!(policy.authorize_ip(ip(address)).is_ok());
    }
    // An ordinary native IPv6 suffix resembling private IPv4 is not a translator.
    assert_eq!(
        policy(true, false).classify(ip("2001:4860::c0a8:102")),
        C::Internet
    );
}
#[test]
fn known_nsp_private_embedding_needs_lan_and_wkp_never_permits_private_embedding() {
    let context = EgressContext::new(vec![], vec![], vec![prefix("2001:4860:abcd::/96")]).unwrap();
    for (internet, lan) in [(true, false), (false, true), (true, true), (false, false)] {
        let policy = EgressPolicy::new(internet, lan, context.clone());
        assert_eq!(policy.classify(ip("2001:4860:abcd::c0a8:102")), C::Lan);
        assert_eq!(
            policy.authorize_ip(ip("2001:4860:abcd::c0a8:102")).is_ok(),
            lan
        );
        assert!(policy.authorize_ip(ip("64:ff9b::c0a8:102")).is_err());
    }
    assert_eq!(
        policy(true, false).classify(ip("64:ff9b::808:808")),
        C::Internet
    );
}
#[test]
fn embedded_host_virtual_and_malformed_layouts_never_bypass_policy() {
    let policy = contextual(&["8.8.4.4"], &[], &["2001:4860::/32"]);
    assert_eq!(
        policy.classify(ip("2001:4860:808:404::")),
        C::Denied(DenyReason::HostAddress)
    );
    for address in ["2001:4860:808:404::1", "2001:4860:808:404:100::"] {
        assert_eq!(
            policy.classify(ip(address)),
            C::Unsupported(UnsupportedReason::MalformedTranslation)
        );
    }
    let policy = contextual(&[], &[], &["2001:4860:abcd::/96"]);
    for address in ["2001:4860:abcd::7f00:1", "2001:4860:abcd::acb:2"] {
        assert!(matches!(policy.classify(ip(address)), C::Denied(_)));
    }
}
#[test]
fn local_use_translation_requires_actual_prefix_and_cannot_erase_outer_restrictions() {
    assert!(matches!(
        policy(true, true).classify(ip("64:ff9b:1:1234:8:808:800:0")),
        C::Unsupported(_)
    ));
    let policy = contextual(&[], &[], &["64:ff9b:1:1234::/64"]);
    assert_eq!(
        policy.classify(ip("64:ff9b:1:1234:8:808:800:0")),
        C::Internet
    );
    let invalid_outer = contextual(&[], &[], &["2001:db8::/96"]);
    assert!(matches!(
        invalid_outer.classify(ip("2001:db8::808:808")),
        C::Denied(_)
    ));
    let unsupported_outer = contextual(&[], &[], &["2002:808:808::/96"]);
    assert!(matches!(
        unsupported_outer.classify(ip("2002:808:808::808:808")),
        C::Unsupported(_)
    ));
}
#[test]
fn private_outer_translator_does_not_let_either_grant_substitute_for_the_other() {
    let context = EgressContext::new(vec![], vec![], vec![prefix("fd12::/96")]).unwrap();
    for (internet, lan) in [(true, false), (false, true), (true, true), (false, false)] {
        let policy = EgressPolicy::new(internet, lan, context.clone());
        assert_eq!(
            policy.authorize_ip(ip("fd12::808:808")).is_ok(),
            internet && lan
        );
    }
}
#[test]
fn private_context_debug_does_not_publish_network_addresses() {
    let policy = contextual(&["192.168.45.67"], &["192.168.45.0/24"], &[]);
    let debug = format!("{policy:?} {:?}", prefix("192.168.45.0/24"));
    assert!(!debug.contains("192.168"));
}
