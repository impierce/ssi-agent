//! An in-process stand-in for the AWS KMS operations the key store uses, signing with real keys, so that
//! identity-kit's key store suite can verify the signatures.

use crate::AwsKmsAmbientKeyProvider;
use aws_config::BehaviorVersion;
use aws_sdk_kms::{
    config::{Builder as KmsConfigBuilder, Credentials, Region},
    Client,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{pkcs8::EncodePublicKey as _, Signer as _};
use p256::ecdsa::signature::hazmat::PrehashSigner as _;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::Duration,
};
use wiremock::{matchers::method, Mock, MockServer, Request, Respond, ResponseTemplate};

const ARN_PREFIX: &str = "arn:aws:kms:eu-central-1:123456789012:key/";

enum SigningKey {
    P256(p256::ecdsa::SigningKey),
    Ed25519(ed25519_dalek::SigningKey),
}

struct FakeKey {
    spec: &'static str,
    /// `None` for a key spec the fake does not sign with.
    signing_key: Option<SigningKey>,
    state: &'static str,
    tags: Vec<(String, String)>,
}

impl FakeKey {
    /// Signs `message` the way AWS KMS does, or `None` when AWS KMS would refuse the request.
    fn sign(&self, message: &[u8], algorithm: Option<&str>, message_type: Option<&str>) -> Option<Vec<u8>> {
        match (self.signing_key.as_ref()?, algorithm, message_type) {
            (SigningKey::P256(key), Some("ECDSA_SHA_256"), Some("DIGEST")) if message.len() == 32 => {
                let signature: p256::ecdsa::Signature = key.sign_prehash(message).ok()?;
                Some(signature.to_der().as_bytes().to_vec())
            }
            (SigningKey::Ed25519(key), Some("ED25519_SHA_512"), Some("RAW")) if message.len() <= 4096 => {
                Some(key.sign(message).to_bytes().to_vec())
            }
            _ => None,
        }
    }

    fn public_key_der(&self) -> Vec<u8> {
        match &self.signing_key {
            Some(SigningKey::P256(key)) => {
                use p256::pkcs8::EncodePublicKey as _;
                key.verifying_key().to_public_key_der().unwrap().as_bytes().to_vec()
            }
            Some(SigningKey::Ed25519(key)) => key.verifying_key().to_public_key_der().unwrap().as_bytes().to_vec(),
            None => Vec::new(),
        }
    }
}

#[derive(Default)]
struct State {
    keys: BTreeMap<String, FakeKey>,
    aliases: BTreeMap<String, String>,
    created: u8,
    refused: BTreeSet<&'static str>,
    /// Operations refused only once a key has been created.
    refused_once_created: BTreeSet<&'static str>,
    /// Operations that take effect but answer with an error, as when their response is lost.
    lost: BTreeSet<&'static str>,
    /// An alias that another key takes just before the next key is created.
    taken_on_create: Option<String>,
    delayed: BTreeMap<&'static str, Duration>,
}

impl State {
    /// The key id an ARN, key id or alias names.
    fn resolve(&self, id: &str) -> Option<String> {
        if id.starts_with("alias/") {
            self.aliases.get(id).cloned()
        } else {
            Some(id.strip_prefix(ARN_PREFIX).unwrap_or(id).to_string()).filter(|id| self.keys.contains_key(id))
        }
    }

    fn metadata(&self, id: &str) -> Value {
        let key = &self.keys[id];
        json!({
            "KeyId": id,
            "Arn": format!("{ARN_PREFIX}{id}"),
            "KeyState": key.state,
            "KeySpec": key.spec,
            "KeyUsage": "SIGN_VERIFY",
        })
    }

    /// Creates a key as a `CreateKey` request `body` asks, after letting another key take an alias if one was set.
    fn create_key(&mut self, body: &Value) -> ResponseTemplate {
        if let Some(alias) = self.taken_on_create.take() {
            self.created += 1;
            let seed = [self.created; 32];
            let other = self.insert(
                "ECC_NIST_P256",
                Some(SigningKey::P256(p256::ecdsa::SigningKey::from_slice(&seed).unwrap())),
            );
            self.aliases.insert(alias, other);
        }
        self.created += 1;
        let seed = [self.created; 32];
        let (spec, signing_key) = match body["KeySpec"].as_str() {
            Some("ECC_NIST_P256") => (
                "ECC_NIST_P256",
                SigningKey::P256(p256::ecdsa::SigningKey::from_slice(&seed).unwrap()),
            ),
            Some("ECC_NIST_EDWARDS25519") => (
                "ECC_NIST_EDWARDS25519",
                SigningKey::Ed25519(ed25519_dalek::SigningKey::from_bytes(&seed)),
            ),
            _ => return error("ValidationException"),
        };
        let id = self.insert(spec, Some(signing_key));
        self.keys.get_mut(&id).unwrap().tags = body["Tags"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|tag| {
                (
                    tag["TagKey"].as_str().unwrap_or_default().to_string(),
                    tag["TagValue"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        ResponseTemplate::new(200).set_body_json(json!({ "KeyMetadata": self.metadata(&id) }))
    }

    fn insert(&mut self, spec: &'static str, signing_key: Option<SigningKey>) -> String {
        let id = format!("00000000-0000-0000-0000-0000000000{:02x}", self.created);
        self.keys.insert(
            id.clone(),
            FakeKey {
                spec,
                signing_key,
                state: "Enabled",
                tags: Vec::new(),
            },
        );
        id
    }
}

pub struct FakeKms {
    server: MockServer,
    state: Arc<Mutex<State>>,
}

impl FakeKms {
    pub async fn start() -> Self {
        let server = MockServer::start().await;
        let state = Arc::<Mutex<State>>::default();
        Mock::given(method("POST"))
            .respond_with(Responder(state.clone()))
            .mount(&server)
            .await;
        Self { server, state }
    }

    pub fn provider(&self) -> AwsKmsAmbientKeyProvider {
        let config = KmsConfigBuilder::default()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("eu-central-1"))
            .credentials_provider(Credentials::new("access-key", "secret-key", None, None, "test"))
            .endpoint_url(self.server.uri())
            .build();
        AwsKmsAmbientKeyProvider::with_client(Client::from_conf(config))
    }

    /// Adds an RSA key, as created outside the key store. Returns its key ID.
    pub fn add_rsa_key(&self) -> String {
        let mut state = self.state.lock().unwrap();
        state.created += 1;
        state.insert("RSA_2048", None)
    }

    /// Puts the key that `id` names in `key_state`, as an operator could in the AWS console.
    pub fn set_key_state(&self, id: &str, key_state: &'static str) {
        let mut state = self.state.lock().unwrap();
        let id = state.resolve(id).unwrap();
        state.keys.get_mut(&id).unwrap().state = key_state;
    }

    /// Makes every request for `operation`, such as `CreateAlias`, fail.
    pub fn refuse(&self, operation: &'static str) {
        self.state.lock().unwrap().refused.insert(operation);
    }

    /// Makes every request for `operation` fail once a key has been created.
    pub fn refuse_once_created(&self, operation: &'static str) {
        self.state.lock().unwrap().refused_once_created.insert(operation);
    }

    /// Carries out every request for `operation`, but answers it with an error, as when its response is lost.
    pub fn lose_responses(&self, operation: &'static str) {
        self.state.lock().unwrap().lost.insert(operation);
    }

    /// Lets another key take `alias` just before the next key is created, as a concurrent request could.
    pub fn take_on_create(&self, alias: &str) {
        self.state.lock().unwrap().taken_on_create = Some(alias.to_string());
    }

    /// The key ID `alias` names, if any.
    pub fn alias_target(&self, alias: &str) -> Option<String> {
        self.state.lock().unwrap().aliases.get(alias).cloned()
    }

    /// The tags of the key `id` names, as key and value.
    pub fn tags(&self, id: &str) -> Vec<(String, String)> {
        let state = self.state.lock().unwrap();
        let id = state.resolve(id).unwrap();
        state.keys[&id].tags.clone()
    }

    /// Answers requests for `operation` again.
    pub fn allow(&self, operation: &'static str) {
        self.state.lock().unwrap().refused.remove(operation);
    }

    /// Answers every request for `operation` only after `delay`.
    pub fn delay(&self, operation: &'static str, delay: Duration) {
        self.state.lock().unwrap().delayed.insert(operation, delay);
    }

    /// Points `alias` at the key `id` names, as retargeting an alias in AWS KMS does.
    pub fn point_alias(&self, alias: &str, id: &str) {
        let mut state = self.state.lock().unwrap();
        let target = state.resolve(id).unwrap();
        state.aliases.insert(alias.to_string(), target);
    }

    /// The state of every key, in the order they were created.
    pub fn key_states(&self) -> Vec<&'static str> {
        self.state.lock().unwrap().keys.values().map(|key| key.state).collect()
    }
}

struct Responder(Arc<Mutex<State>>);

fn error(code: &str) -> ResponseTemplate {
    ResponseTemplate::new(400)
        .insert_header("x-amzn-ErrorType", code)
        .set_body_json(json!({ "__type": code, "message": "rejected by the fake" }))
}

fn blob(value: &Value, field: &str) -> Vec<u8> {
    STANDARD
        .decode(value[field].as_str().unwrap_or_default())
        .unwrap_or_default()
}

fn operation(request: &Request) -> String {
    request
        .headers
        .get("x-amz-target")
        .and_then(|target| target.to_str().ok())
        .and_then(|target| target.strip_prefix("TrentService."))
        .unwrap_or_default()
        .to_string()
}

impl Respond for Responder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let operation = operation(request);
        let delay = self.0.lock().unwrap().delayed.get(operation.as_str()).copied();
        let lost = self.0.lock().unwrap().lost.contains(operation.as_str());
        let response = self.answer(&operation, request);
        let response = if lost { error("KMSInternalException") } else { response };
        match delay {
            Some(delay) => response.set_delay(delay),
            None => response,
        }
    }
}

impl Responder {
    fn answer(&self, operation: &str, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap_or_default();
        let mut state = self.0.lock().unwrap();

        if state.refused.contains(operation) || (state.created > 0 && state.refused_once_created.contains(operation)) {
            return error("LimitExceededException");
        }

        if operation == "CreateKey" {
            return state.create_key(&body);
        }

        if operation == "CreateAlias" {
            let alias = body["AliasName"].as_str().unwrap_or_default().to_string();
            if state.aliases.contains_key(&alias) {
                return error("AlreadyExistsException");
            }
            let Some(target) = state.resolve(body["TargetKeyId"].as_str().unwrap_or_default()) else {
                return error("NotFoundException");
            };
            state.aliases.insert(alias, target);
            return ResponseTemplate::new(200).set_body_json(json!({}));
        }

        if operation == "ListAliases" {
            let key = state.resolve(body["KeyId"].as_str().unwrap_or_default());
            let aliases: Vec<Value> = state
                .aliases
                .iter()
                .filter(|(_, target)| Some(*target) == key.as_ref())
                .map(|(alias, target)| json!({ "AliasName": alias, "TargetKeyId": target }))
                .collect();
            return ResponseTemplate::new(200).set_body_json(json!({ "Aliases": aliases, "Truncated": false }));
        }

        if operation == "DeleteAlias" {
            return match state.aliases.remove(body["AliasName"].as_str().unwrap_or_default()) {
                Some(_) => ResponseTemplate::new(200).set_body_json(json!({})),
                None => error("NotFoundException"),
            };
        }

        let Some(id) = state.resolve(body["KeyId"].as_str().unwrap_or_default()) else {
            return error("NotFoundException");
        };
        match (operation, state.keys[&id].state) {
            ("DescribeKey", _) => {
                ResponseTemplate::new(200).set_body_json(json!({ "KeyMetadata": state.metadata(&id) }))
            }
            (_, "PendingDeletion") => error("KMSInvalidStateException"),
            ("ScheduleKeyDeletion", _) => {
                state.keys.get_mut(&id).unwrap().state = "PendingDeletion";
                ResponseTemplate::new(200).set_body_json(json!({
                    "KeyId": format!("{ARN_PREFIX}{id}"),
                    "KeyState": "PendingDeletion",
                    "PendingWindowInDays": 30,
                }))
            }
            (_, "Disabled") => error("DisabledException"),
            ("GetPublicKey", _) => ResponseTemplate::new(200).set_body_json(json!({
                "KeyId": format!("{ARN_PREFIX}{id}"),
                "PublicKey": STANDARD.encode(state.keys[&id].public_key_der()),
                "KeySpec": state.keys[&id].spec,
                "KeyUsage": "SIGN_VERIFY",
            })),
            ("Sign", _) => {
                let Some(signature) = state.keys[&id].sign(
                    &blob(&body, "Message"),
                    body["SigningAlgorithm"].as_str(),
                    body["MessageType"].as_str(),
                ) else {
                    return error("ValidationException");
                };
                ResponseTemplate::new(200).set_body_json(json!({
                    "KeyId": format!("{ARN_PREFIX}{id}"),
                    "Signature": STANDARD.encode(signature),
                    "SigningAlgorithm": body["SigningAlgorithm"],
                }))
            }
            _ => error("UnsupportedOperationException"),
        }
    }
}
