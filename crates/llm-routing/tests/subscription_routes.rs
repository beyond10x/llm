//! story:anthropic-access: a catalog never lets a subscription token travel as metered use.
//!
//! Two refusals, both made by catalog validation before any secret is resolved:
//!
//! - a `subscription-oauth` account whose secret reference any other account also names, because
//!   that other account would present the same token under another kind or billing;
//! - a route with fallback that holds a `subscription-oauth` target and any target not billed as
//!   `subscription`, in either order, because fallback would move a turn between the two.
//!
//! A route without fallback may hold both: only its first target is ever selected.
use llm_core::{AuthKind, BillingKind, ErrorCode, Id, Item, TurnRequest};
use llm_routing::Catalog;

const SHARED: &str =
    "a subscription OAuth account cannot share its secret reference with another account";
const MIXED: &str = "a route with fallback cannot mix a subscription OAuth target with a target under other billing";

/// The second account, as its declaration lines.
struct Other {
    auth_kind: &'static str,
    billing_kind: &'static str,
    reference: Option<&'static str>,
}

impl Other {
    fn lines(&self) -> String {
        let mut lines = format!(
            "auth_kind = \"{}\"\nbilling_kind = \"{}\"\n",
            self.auth_kind, self.billing_kind
        );
        if let Some(reference) = self.reference {
            lines += "secret_reference_id = \"";
            lines += reference;
            lines += "\"\n";
        }
        if self.auth_kind == "api-key" {
            lines.push_str("api_key_header = \"x-api-key\"\n");
        }
        lines
    }
}

const METERED_KEY: Other = Other {
    auth_kind: "api-key",
    billing_kind: "metered",
    reference: Some("metered-key"),
};

/// One subscription account and one other, each with its own Messages serving model; one route
/// over both in the order given. `other_first` puts the other account's declaration first.
fn catalog(other: &Other, fallback: bool, order: [&str; 2], other_first: bool) -> String {
    let subscription = "[[accounts]]\nid = \"operator\"\nprovider_id = \"anthropic\"\nauth_kind = \"subscription-oauth\"\nbilling_kind = \"subscription\"\nsecret_reference_id = \"operator-subscription\"\n";
    let second = format!(
        "[[accounts]]\nid = \"other\"\nprovider_id = \"anthropic\"\n{}",
        other.lines()
    );
    let accounts = if other_first {
        format!("{second}\n{subscription}")
    } else {
        format!("{subscription}\n{second}")
    };
    let serving = |name: &str, endpoint: &str| {
        format!(
            "[[serving_models]]\nid = \"{name}-model\"\nendpoint_id = \"{endpoint}\"\nmodel_id = \"internal-model\"\nprotocol = \"messages\"\n[serving_models.capabilities]\ntools = true\ntool_choice = true\ntemperature = true\ntop_p = false\nreasoning_efforts = []\ncontext_window = 200000\nmax_output_tokens = 8192\n"
        )
    };
    format!(
        "format = \"llm.catalog/1\"\n\n[[providers]]\nid = \"anthropic\"\ncategory = \"hosted\"\n\n{accounts}\n\
[[endpoints]]\nid = \"subscription-messages\"\naccount_id = \"operator\"\nbase_url = \"https://messages.example.invalid/v1\"\n\n\
[[endpoints]]\nid = \"other-messages\"\naccount_id = \"other\"\nbase_url = \"https://messages.example.invalid/v1\"\n\n\
[[models]]\nid = \"internal-model\"\nupstream_name = \"example/Model-Revision\"\n\n{}\n{}\n\
[[routes]]\nid = \"assistant\"\nalias = \"assist\"\nfallback_enabled = {fallback}\n\n\
[[targets]]\nid = \"{first}-target\"\nroute_id = \"assistant\"\nserving_model_id = \"{first}-model\"\nposition = 0\n\n\
[[targets]]\nid = \"{second}-target\"\nroute_id = \"assistant\"\nserving_model_id = \"{second}-model\"\nposition = 1\n",
        serving("subscription", "subscription-messages"),
        serving("other", "other-messages"),
        first = order[0],
        second = order[1],
    )
}

fn refused(source: &str, message: &str) {
    let error = Catalog::parse(source).err().unwrap_or_else(|| {
        panic!("the catalog is refused with `{message}`, and it was accepted:\n{source}")
    });
    assert_eq!(error.code, ErrorCode::InvalidRequest, "{error}");
    assert_eq!(error.message, message);
}

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("assist", vec![Item::user("Summarise the log")]);
    request.max_output_tokens = Some(512);
    request
}

#[test]
fn a_subscription_reference_shared_with_any_other_account_is_refused_in_either_order() {
    for (auth_kind, billing_kind) in [
        ("bearer", "metered"),
        ("api-key", "metered"),
        ("bearer", "subscription"),
        ("subscription-oauth", "subscription"),
    ] {
        let other = Other {
            auth_kind,
            billing_kind,
            reference: Some("operator-subscription"),
        };
        for other_first in [false, true] {
            refused(
                &catalog(&other, false, ["subscription", "other"], other_first),
                SHARED,
            );
        }
    }
}

#[test]
fn a_falling_back_route_mixing_subscription_and_other_billing_is_refused_in_either_order() {
    let self_hosted = Other {
        auth_kind: "anonymous",
        billing_kind: "self-hosted",
        reference: None,
    };
    let metered_bearer = Other {
        auth_kind: "bearer",
        billing_kind: "metered",
        reference: Some("metered-token"),
    };
    for other in [&METERED_KEY, &self_hosted, &metered_bearer] {
        for order in [["subscription", "other"], ["other", "subscription"]] {
            refused(&catalog(other, true, order, false), MIXED);
        }
    }
}

#[test]
fn a_route_without_fallback_may_hold_a_subscription_and_a_metered_target() {
    let catalog = Catalog::parse(&catalog(
        &METERED_KEY,
        false,
        ["subscription", "other"],
        false,
    ))
    .expect("without fallback only the first target is ever selected");
    let selected = catalog.resolve(&request(), Some(100)).expect("selected");
    assert_eq!(selected.target.id, Id::new("subscription-target").unwrap());
    let account = &selected.binding.declaration().account;
    assert_eq!(account.auth_kind, AuthKind::SubscriptionOauth);
    assert_eq!(account.billing_kind, BillingKind::Subscription);
}

#[test]
fn a_falling_back_route_between_subscription_billed_targets_is_accepted() {
    let other = Other {
        auth_kind: "bearer",
        billing_kind: "subscription",
        reference: Some("other-subscription"),
    };
    for order in [["subscription", "other"], ["other", "subscription"]] {
        Catalog::parse(&catalog(&other, true, order, false))
            .expect("no target of this route is billed as anything but a subscription");
    }
}
