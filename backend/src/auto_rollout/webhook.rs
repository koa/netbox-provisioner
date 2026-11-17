//! Scope: Whether a call of the NetBox webhook is NetBox's: its signature; not what follows
//! from it (`auto_rollout`).
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha512;

/// Whether `signature` (NetBox' `X-Hook-Signature`: HMAC-SHA512 of the body with the
/// webhook's secret, hex) is `body`'s with `secret`.
pub fn signed(secret: &str, body: &[u8], signature: &str) -> bool {
    let Some(expected) = from_hex(signature.trim()) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha512>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

fn from_hex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod test {
    use super::signed;

    /// The signature NetBox sends, worked out as NetBox does (`hmac.new(secret, body,
    /// sha512).hexdigest()`, Python, for this body and secret).
    #[test]
    fn netbox_signature_is_checked() {
        let body = br#"{"event": "updated"}"#;
        let signature = "908fdf4e42cd6231a91abe69a555e788b1ec8bef45c8c352972b0a032542c6b7\
                         8a815d786f936b3152cded524ec58dce85a5ae84a56fbb044fc7c2ba31b586d0";
        assert!(signed("secret", body, signature));
        assert!(!signed("other", body, signature));
        assert!(!signed("secret", br#"{"event": "deleted"}"#, signature));
        assert!(!signed("secret", body, "zz"));
    }
}
