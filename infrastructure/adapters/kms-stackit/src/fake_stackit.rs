//! An in-process stand-in for STACKIT's sign-in and the KMS API of one key ring, signing with real P-256 keys, so that
//! identity-kit's key store suite can verify the signatures.

use crate::sign_in::{tests::TokenFile, SignIn};
use crate::StackitKmsKeyProvider;
use agent_kms::key_provider::settings::{StackitKmsSettings, StackitRegion};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use p256::ecdsa::signature::Signer as _;
use p256::pkcs8::EncodePublicKey as _;
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use url::Url;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

pub(crate) const PROJECT_ID: &str = "4a1d2f6e-0f33-4c55-9b56-6f8e8a2b1c3d";
pub(crate) const KEY_RING_ID: &str = "9c0e7b52-5d2a-4f0e-8a51-3b6d1e2f4a7c";
pub(crate) const ES256: &str = "ecdsa_p256_sha256";

pub(crate) struct FakeVersion {
    pub(crate) state: &'static str,
    pub(crate) disabled: bool,
    signing_key: p256::ecdsa::SigningKey,
}

pub(crate) struct FakeKey {
    pub(crate) id: String,
    display_name: String,
    algorithm: &'static str,
    pub(crate) state: &'static str,
    pub(crate) versions: Vec<FakeVersion>,
}

impl FakeKey {
    fn json(&self) -> Value {
        json!({
            "id": self.id,
            "displayName": self.display_name,
            "algorithm": self.algorithm,
            "purpose": "asymmetric_sign_verify",
            "protection": "software",
            "state": self.state,
            "keyRingId": KEY_RING_ID,
            "createdAt": "2026-11-23T12:00:00Z",
            "importOnly": false,
            "access_scope": "PUBLIC",
        })
    }

    fn version_json(&self, index: usize) -> Value {
        let version = &self.versions[index];
        let public_key = version.signing_key.verifying_key().to_public_key_der().unwrap();
        json!({
            "number": index + 1,
            "keyId": self.id,
            "keyRingId": KEY_RING_ID,
            "state": version.state,
            "disabled": version.disabled,
            "createdAt": "2026-11-23T12:00:00Z",
            "publicKey": pem(public_key.as_bytes()),
        })
    }
}

/// A PEM public key, as STACKIT returns it.
pub(crate) fn pem(der: &[u8]) -> String {
    let body = STANDARD.encode(der);
    let lines: Vec<&str> = body
        .as_bytes()
        .chunks(64)
        .map(|line| std::str::from_utf8(line).unwrap())
        .collect();
    format!(
        "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
        lines.join("\n")
    )
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) keys: Vec<FakeKey>,
    created: u8,
    /// How many times a new key's versions are listed before its first version can sign. `None` for never.
    pub(crate) polls_until_usable: Option<usize>,
    polls: usize,
    /// How many of the next KMS requests are refused as unauthorized.
    pub(crate) refused_tokens: usize,
    /// Whether every deletion fails.
    pub(crate) deletion_fails: bool,
    sign_ins: usize,
}

impl State {
    fn new_key(&mut self, display_name: &str, algorithm: &'static str, state: &'static str) -> &mut FakeKey {
        self.created += 1;
        self.keys.push(FakeKey {
            id: format!("00000000-0000-4000-8000-{:012}", self.created),
            display_name: display_name.to_string(),
            algorithm,
            state,
            versions: Vec::new(),
        });
        self.keys.last_mut().unwrap()
    }

    fn new_version(&mut self, key: usize, state: &'static str, disabled: bool) {
        self.created += 1;
        let signing_key = p256::ecdsa::SigningKey::from_slice(&[self.created; 32]).unwrap();
        self.keys[key].versions.push(FakeVersion {
            state,
            disabled,
            signing_key,
        });
    }

    fn key(&self, id: &str) -> Option<usize> {
        self.keys.iter().position(|key| key.id == id)
    }

    fn respond(&mut self, request: &Request) -> ResponseTemplate {
        let path: Vec<&str> = request.url.path_segments().unwrap().collect();
        if path == ["token"] {
            self.sign_ins += 1;
            return ResponseTemplate::new(200).set_body_json(json!({
                "access_token": format!("token-{}", self.sign_ins),
                "token_type": "Bearer",
                "expires_in": 900
            }));
        }
        if !request
            .headers
            .get("authorization")
            .is_some_and(|value| value.to_str().unwrap().starts_with("Bearer token-"))
        {
            return error(401, "no access token");
        }
        if self.refused_tokens > 0 {
            self.refused_tokens -= 1;
            return error(401, "the access token expired");
        }

        let method = request.method.as_str();
        let below_key_ring = match path.as_slice() {
            ["v1", "projects", PROJECT_ID, "regions", "eu01", "keyrings", key_ring, rest @ ..]
                if *key_ring == KEY_RING_ID =>
            {
                rest
            }
            ["v1", "projects", _, "regions", _, "keyrings", _, ..] => return error(404, "key ring not found"),
            _ => return error(404, "no such route"),
        };
        match (method, below_key_ring) {
            ("GET", []) => ResponseTemplate::new(200).set_body_json(json!({
                "id": KEY_RING_ID, "displayName": "keys", "state": "active", "createdAt": "2026-11-23T12:00:00Z"
            })),
            ("GET", ["keys"]) => {
                let keys: Vec<Value> = self.keys.iter().map(FakeKey::json).collect();
                ResponseTemplate::new(200).set_body_json(json!({ "keys": keys }))
            }
            ("POST", ["keys"]) => {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                if body["algorithm"] != ES256
                    || body["purpose"] != "asymmetric_sign_verify"
                    || body["protection"] != "software"
                {
                    return error(400, "the fake only creates software-protected ES256 signing keys");
                }
                let usable = self.polls_until_usable == Some(0);
                let display_name = body["displayName"].as_str().unwrap().to_string();
                let key = self
                    .new_key(&display_name, ES256, if usable { "active" } else { "creating" })
                    .json();
                let index = self.keys.len() - 1;
                self.new_version(index, if usable { "active" } else { "creating" }, false);
                self.polls = 0;
                ResponseTemplate::new(202).set_body_json(key)
            }
            (method, ["keys", id, rest @ ..]) => {
                let Some(index) = self.key(id) else {
                    return error(404, "key not found");
                };
                self.respond_for_key(method, index, rest, request)
            }
            _ => error(404, "no such route"),
        }
    }

    fn respond_for_key(&mut self, method: &str, index: usize, path: &[&str], request: &Request) -> ResponseTemplate {
        match (method, path) {
            ("GET", []) => ResponseTemplate::new(200).set_body_json(self.keys[index].json()),
            ("DELETE", []) if self.deletion_fails => error(500, "deletion failed"),
            ("DELETE", []) if self.keys[index].state == "deleted" => error(409, "the key is already deleted"),
            ("DELETE", []) => {
                self.keys[index].state = "deleted";
                ResponseTemplate::new(202)
            }
            ("GET", ["versions"]) => {
                self.ripen(index);
                let key = &self.keys[index];
                let versions: Vec<Value> = (0..key.versions.len())
                    .map(|version| key.version_json(version))
                    .collect();
                ResponseTemplate::new(200).set_body_json(json!({ "versions": versions }))
            }
            ("GET", ["versions", number]) => match self.version(index, number) {
                Some(version) => ResponseTemplate::new(200).set_body_json(self.keys[index].version_json(version)),
                None => error(404, "version not found"),
            },
            ("POST", ["versions", number, "sign"]) => {
                let Some(version) = self.version(index, number) else {
                    return error(404, "version not found");
                };
                let key = &self.keys[index];
                let version = &key.versions[version];
                if key.state != "active" || version.state != "active" || version.disabled {
                    return error(400, "the key version can't sign");
                }
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                let data = STANDARD.decode(body["data"].as_str().unwrap()).unwrap();
                let signature: p256::ecdsa::Signature = version.signing_key.sign(&data);
                ResponseTemplate::new(200).set_body_json(json!({
                    "data": body["data"],
                    "signature": STANDARD.encode(signature.to_der().as_bytes())
                }))
            }
            _ => error(404, "no such route"),
        }
    }

    /// A new key's first version can sign once its versions were listed often enough.
    fn ripen(&mut self, index: usize) {
        let key = &mut self.keys[index];
        if key.state != "creating" {
            return;
        }
        if self.polls_until_usable.is_some_and(|polls| self.polls >= polls) {
            key.state = "active";
            key.versions[0].state = "active";
        }
        self.polls += 1;
    }

    fn version(&self, index: usize, number: &str) -> Option<usize> {
        let number: usize = number.parse().ok()?;
        (1..=self.keys[index].versions.len())
            .contains(&number)
            .then(|| number - 1)
    }
}

fn error(status: u16, message: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({ "message": message }))
}

struct Responder(Arc<Mutex<State>>);

impl Respond for Responder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        self.0.lock().unwrap().respond(request)
    }
}

pub(crate) struct FakeStackit {
    pub(crate) server: MockServer,
    pub(crate) state: Arc<Mutex<State>>,
    token_file: TokenFile,
}

impl FakeStackit {
    /// A fake whose new keys can sign at once.
    pub(crate) async fn start() -> Self {
        let server = MockServer::start().await;
        let state = Arc::new(Mutex::new(State {
            polls_until_usable: Some(0),
            ..State::default()
        }));
        Mock::given(wiremock::matchers::any())
            .respond_with(Responder(state.clone()))
            .mount(&server)
            .await;
        Self {
            server,
            state,
            token_file: TokenFile::new(),
        }
    }

    /// The provider for the fake's key ring, waiting at most 300 milliseconds for a new key's version.
    pub(crate) fn provider(&self) -> StackitKmsKeyProvider {
        self.provider_for(KEY_RING_ID)
    }

    pub(crate) fn provider_for(&self, key_ring_id: &str) -> StackitKmsKeyProvider {
        let settings = StackitKmsSettings {
            project_id: PROJECT_ID.to_string(),
            region: StackitRegion::Eu01,
            key_ring_id: key_ring_id.to_string(),
            service_account_email: "unicore-kms@sa.stackit.cloud".to_string(),
        };
        let sign_in = SignIn::WorkloadIdentity {
            token_file: self.token_file.0.clone(),
            service_account_email: settings.service_account_email.clone(),
            token_url: Url::parse(&format!("{}/token", self.server.uri())).unwrap(),
        };
        let mut provider =
            StackitKmsKeyProvider::new(&Url::parse(&self.server.uri()).unwrap(), &settings, sign_in).unwrap();
        provider.version_wait = Duration::from_millis(300);
        provider.poll_interval = Duration::from_millis(10);
        provider
    }

    /// Adds a key with one version per `versions` entry (state, disabled), and returns its id.
    pub(crate) fn add_key(
        &self,
        display_name: &str,
        algorithm: &'static str,
        state: &'static str,
        versions: &[(&'static str, bool)],
    ) -> String {
        let mut fake = self.state.lock().unwrap();
        let id = fake.new_key(display_name, algorithm, state).id.clone();
        let index = fake.keys.len() - 1;
        for (version_state, disabled) in versions {
            fake.new_version(index, version_state, *disabled);
        }
        id
    }

    /// Adds an active ES256 key with one active version, and returns its id.
    pub(crate) fn add_active_key(&self, display_name: &str) -> String {
        self.add_key(display_name, ES256, "active", &[("active", false)])
    }

    pub(crate) fn key_state(&self, id: &str) -> &'static str {
        let fake = self.state.lock().unwrap();
        fake.keys[fake.key(id).unwrap()].state
    }

    /// The public key of a version, as a JWK.
    pub(crate) fn public_jwk(&self, id: &str, number: usize) -> identity_kit::jose::Jwk {
        let fake = self.state.lock().unwrap();
        let key = &fake.keys[fake.key(id).unwrap()];
        let der = key.versions[number - 1]
            .signing_key
            .verifying_key()
            .to_public_key_der()
            .unwrap();
        identity_kit::jose::Jwk::from_spki_der(identity_kit::jose::KeyType::P256, der.as_bytes()).unwrap()
    }

    /// The requests sent to the KMS API, leaving out sign-ins.
    pub(crate) async fn kms_requests(&self) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path() != "/token")
            .count()
    }

    pub(crate) fn sign_ins(&self) -> usize {
        self.state.lock().unwrap().sign_ins
    }
}
