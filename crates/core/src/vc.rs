//! Open Badges 3.0 Verifiable Credential builder.
//!
//! Constructs the unsigned JSON-LD document for an OB 3.0 credential, ready
//! to be passed to `did::sign_document`. The signed VC is then persisted to
//! `credentials.vc_json` and becomes the source of truth for the credential.
//!
//! See: <https://www.imsglobal.org/spec/ob/v3p0>
//! See: CredBrew v1.0 Design RFC §5

use crate::error::CoreError;
use crate::models::{Event, Participant, ParticipantType, TemplateKind};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

/// Inputs needed to build an OB 3.0 credential. The caller (worker) gathers
/// these from the database; the builder is pure — no I/O, no signing.
pub struct VcInputs<'a> {
    pub credential_id: Uuid,
    pub issuer_did: &'a str,
    pub issuer_name: &'a str,
    pub public_base_url: &'a str,
    pub participant: &'a Participant,
    pub participant_type: &'a ParticipantType,
    pub event: &'a Event,
    pub template_kind: TemplateKind,
    pub status_list_credential_url: &'a str,
    pub status_list_index: u32,
}

/// The unsigned OB 3.0 Verifiable Credential as a JSON Value, ready for
/// `did::sign_document` to attach an Ed25519Signature2020 proof.
pub fn build_ob30_credential(inputs: &VcInputs) -> Result<Value, CoreError> {
    let credential_url = format!(
        "{}/api/public/vc/{}",
        inputs.public_base_url.trim_end_matches('/'),
        inputs.credential_id
    );

    let achievement_id = format!(
        "{}/achievements/{}-{}",
        inputs.public_base_url.trim_end_matches('/'),
        slugify(&inputs.participant_type.name),
        inputs.event.id.simple()
    );

    let (kind_label, achievement_name) = match inputs.template_kind {
        TemplateKind::Badge => (
            "badge",
            format!("{} — {}", inputs.participant_type.name, inputs.event.name),
        ),
        TemplateKind::Certificate => (
            "certificate",
            format!(
                "{} — {} (Certificate)",
                inputs.participant_type.name, inputs.event.name
            ),
        ),
    };

    // The credentialSubject.id is a mailto: URL derived from the participant's
    // email. This is the OB 3.0-recommended identifier for a recipient who
    // does not have their own DID. A future phase can issue to a recipient-
    // provided DID instead.
    let subject_id = format!("mailto:{}", inputs.participant.email);

    let vc = json!({
        "@context": [
            "https://www.w3.org/2018/credentials/v1",
            "https://purl.imsglobal.org/spec/ob/v3p0/context.json"
        ],
        "id": credential_url,
        "type": ["VerifiableCredential", "OpenBadgeCredential"],
        "issuer": {
            "id": inputs.issuer_did,
            "type": "Profile",
            "name": inputs.issuer_name
        },
        "issuanceDate": Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        "credentialSubject": {
            "type": "AchievementSubject",
            "id": subject_id,
            "name": inputs.participant.name,
            "achievement": {
                "id": achievement_id,
                "type": "Achievement",
                "name": achievement_name,
                "kind": kind_label,
                "criteria": {
                    "narrative": format!(
                        "Awarded to {} as a {} at {}",
                        inputs.participant.name,
                        inputs.participant_type.name,
                        inputs.event.name
                    )
                },
                "event": {
                    "type": "Event",
                    "name": inputs.event.name,
                    "startDate": inputs.event.event_date
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .unwrap_or_else(|| inputs.event.created_at.format("%Y-%m-%d").to_string()),
                    "organizer": {
                        "type": "Profile",
                        "name": if inputs.event.org_name.is_empty() {
                            inputs.issuer_name
                        } else {
                            &inputs.event.org_name
                        }
                    }
                }
            }
        },
        "credentialStatus": {
            "id": format!("{}#{}", inputs.status_list_credential_url, inputs.status_list_index),
            "type": "BitstringStatusListEntry",
            "statusPurpose": "revocation",
            "statusListIndex": inputs.status_list_index,
            "statusListCredential": inputs.status_list_credential_url
        }
    });

    Ok(vc)
}

/// Build the URL of the StatusListCredential for a tenant's status list at a
/// given index. The list itself is served at this URL as a Verifiable
/// Credential (signed by the tenant's DID). Phase 4 implements the signing;
/// Phase 3 just stores the URL reference.
pub fn status_list_credential_url(
    public_base_url: &str,
    tenant_slug: &str,
    list_index: i32,
) -> String {
    format!(
        "{}/tenants/{}/status/{}",
        public_base_url.trim_end_matches('/'),
        tenant_slug,
        list_index
    )
}

/// The signed Verifiable Credential (vc_json) plus the metadata needed to
/// persist it on the credentials row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedVc {
    pub vc_id: String,
    pub vc_json: Value,
    pub status_list_index: u32,
}

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' => c,
            _ => '-',
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Event, Participant, ParticipantType};
    use chrono::NaiveDate;

    fn fake_event() -> Event {
        Event {
            id: Uuid::new_v4(),
            owner_id: Uuid::new_v4(),
            tenant_id: None,
            name: "TechConf 2025".to_string(),
            event_date: NaiveDate::from_ymd_opt(2025, 6, 15),
            org_name: "TechConf Org".to_string(),
            accent_color: "#4F46E5".to_string(),
            created_at: Utc::now(),
        }
    }

    fn fake_participant_type() -> ParticipantType {
        ParticipantType {
            id: Uuid::new_v4(),
            event_id: Uuid::nil(),
            name: "Speaker".to_string(),
            badge_enabled: true,
            certificate_enabled: true,
        }
    }

    fn fake_participant() -> Participant {
        Participant {
            id: Uuid::new_v4(),
            event_id: Uuid::nil(),
            participant_type_id: Uuid::nil(),
            name: "Alice Chen".to_string(),
            email: "alice@example.com".to_string(),
            short_code: "ABC12345".to_string(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn test_build_ob30_credential_shape() {
        let cred_id = Uuid::new_v4();
        let event = fake_event();
        let pt = fake_participant_type();
        let participant = fake_participant();
        let status_url = "https://credbrew.example/tenants/techconf/status/1";

        let inputs = VcInputs {
            credential_id: cred_id,
            issuer_did: "did:web:credbrew.example:tenants:techconf",
            issuer_name: "TechConf",
            public_base_url: "https://credbrew.example",
            participant: &participant,
            participant_type: &pt,
            event: &event,
            template_kind: TemplateKind::Badge,
            status_list_credential_url: status_url,
            status_list_index: 42,
        };

        let vc = build_ob30_credential(&inputs).unwrap();

        // Required W3C VC fields
        assert!(vc["@context"].is_array());
        assert_eq!(vc["type"][0], "VerifiableCredential");
        assert_eq!(vc["type"][1], "OpenBadgeCredential");
        assert_eq!(
            vc["issuer"]["id"],
            "did:web:credbrew.example:tenants:techconf"
        );
        assert_eq!(vc["issuer"]["name"], "TechConf");
        assert!(vc["issuanceDate"].is_string());

        // OB 3.0 credentialSubject
        assert_eq!(vc["credentialSubject"]["type"], "AchievementSubject");
        assert_eq!(vc["credentialSubject"]["id"], "mailto:alice@example.com");
        assert_eq!(vc["credentialSubject"]["name"], "Alice Chen");
        assert_eq!(
            vc["credentialSubject"]["achievement"]["name"],
            "Speaker — TechConf 2025"
        );
        assert_eq!(vc["credentialSubject"]["achievement"]["kind"], "badge");

        // Status list entry
        assert_eq!(vc["credentialStatus"]["type"], "BitstringStatusListEntry");
        assert_eq!(vc["credentialStatus"]["statusPurpose"], "revocation");
        assert_eq!(vc["credentialStatus"]["statusListIndex"], 42);
        assert_eq!(vc["credentialStatus"]["statusListCredential"], status_url);

        // The credential id is a URL on the issuer's domain
        let id = vc["id"].as_str().unwrap();
        assert!(id.starts_with("https://credbrew.example/api/public/vc/"));
        assert!(id.ends_with(&cred_id.to_string()));
    }

    #[test]
    fn test_certificate_kind_adds_label() {
        let event = fake_event();
        let pt = fake_participant_type();
        let participant = fake_participant();

        let inputs = VcInputs {
            credential_id: Uuid::new_v4(),
            issuer_did: "did:web:example:test",
            issuer_name: "Test",
            public_base_url: "https://example.com",
            participant: &participant,
            participant_type: &pt,
            event: &event,
            template_kind: TemplateKind::Certificate,
            status_list_credential_url: "https://example.com/status/1",
            status_list_index: 0,
        };

        let vc = build_ob30_credential(&inputs).unwrap();
        assert_eq!(
            vc["credentialSubject"]["achievement"]["kind"],
            "certificate"
        );
        assert!(vc["credentialSubject"]["achievement"]["name"]
            .as_str()
            .unwrap()
            .contains("(Certificate)"));
    }

    #[test]
    fn test_status_list_url_construction() {
        let url = status_list_credential_url("https://credbrew.example/", "techconf", 1);
        assert_eq!(url, "https://credbrew.example/tenants/techconf/status/1");
    }

    #[test]
    fn test_build_then_sign_then_verify() {
        // End-to-end: build an unsigned VC, sign it with a fresh Ed25519 key,
        // verify it back. Proves the vc.rs + did.rs integration.
        use crate::did::{generate_keypair, sign_document, signing_key_from_seed, verify_document};

        let kp = generate_keypair();
        let signing_key = signing_key_from_seed(&kp.seed);
        let did = "did:web:credbrew.example:tenants:test";
        let vm = format!("{did}#key-1");

        let event = fake_event();
        let pt = fake_participant_type();
        let participant = fake_participant();

        let inputs = VcInputs {
            credential_id: Uuid::new_v4(),
            issuer_did: did,
            issuer_name: "Test",
            public_base_url: "https://credbrew.example",
            participant: &participant,
            participant_type: &pt,
            event: &event,
            template_kind: TemplateKind::Badge,
            status_list_credential_url: "https://credbrew.example/tenants/test/status/1",
            status_list_index: 0,
        };

        let unsigned = build_ob30_credential(&inputs).unwrap();
        let (proof, signed) = sign_document(&unsigned, &vm, &signing_key).unwrap();
        assert!(proof.proof_value.is_some());

        // Verify with the correct public key
        let ok = verify_document(&signed, &kp.public_key).unwrap();
        assert!(ok, "signed VC must verify with the issuer's public key");

        // Tamper with the achievement name
        let mut tampered = signed.clone();
        tampered["credentialSubject"]["achievement"]["name"] = json!("Mallory's Fake Badge");
        let ok_tampered = verify_document(&tampered, &kp.public_key).unwrap();
        assert!(!ok_tampered, "tampered VC must fail verification");
    }
}
