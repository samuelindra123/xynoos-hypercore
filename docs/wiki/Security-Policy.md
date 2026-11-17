# Security Policy

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

The Debian archive uses a dedicated OpenPGP trust anchor: HTTPS delivers its public key; an independently confirmed full fingerprint establishes identity; per-source signed-by limits trust. Release signatures bind index SHA256 values and package SHA256 records. GitHub OIDC artifact attestations separately bind exact workflow/source subjects. The existing standalone installer retains a separate pinned Ed25519 identity.

Private APT signing material belongs only in protected Actions secrets or an approved signing service. Never commit/upload/log it. CI uses ephemeral explicitly labeled TEST keys and never publishes them. Invalid signatures, modified metadata/packages, missing keys, duplicate versions and wrong architecture fail closed. No apt-key or signature bypass is supported.

For a suspected vulnerability, use [GitHub private vulnerability reporting](https://github.com/samuelindra123/xynoos-hypercore/security/advisories/new) if enabled; if unavailable, contact the maintainer using the repository owner's public profile without disclosing exploit details publicly. Report packaging integrity failures with hashes/run IDs while redacting private paths and evidence. See [security guidance](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/debian/SECURITY.md).
