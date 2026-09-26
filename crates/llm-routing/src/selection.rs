use crate::{Catalog, RouteTarget};
use llm_core::{AuthKind, BillingKind, Capabilities, Error, Id, Item, Provenance, TurnRequest};
use llm_providers::Binding;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Rejection {
    Tools,
    ToolChoice,
    Temperature,
    TopP,
    ReasoningEffort,
    OutputLimit,
    InputTokensUnknown,
    ContextWindow,
    OpaqueState,
    FallbackDisabled,
}
impl Rejection {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tools => "tools",
            Self::ToolChoice => "tool-choice",
            Self::Temperature => "temperature",
            Self::TopP => "top-p",
            Self::ReasoningEffort => "reasoning-effort",
            Self::OutputLimit => "output-limit",
            Self::InputTokensUnknown => "input-tokens-unknown",
            Self::ContextWindow => "context-window",
            Self::OpaqueState => "opaque-state",
            Self::FallbackDisabled => "fallback-disabled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetExplanation {
    pub target_id: Id,
    pub serving_model_id: Id,
    pub position: usize,
    pub provenance: Provenance,
    pub auth_kind: AuthKind,
    pub billing_kind: BillingKind,
    pub capabilities: Capabilities,
    pub rejections: Vec<Rejection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteExplanation {
    pub route_id: Id,
    pub alias: Id,
    pub config_digest: String,
    pub input_tokens: Option<u64>,
    pub selected_target_id: Option<Id>,
    pub targets: Vec<TargetExplanation>,
}

/// A pure selection, not an executed attempt or permission to retry after failure.
#[derive(Debug)]
pub struct Selection<'a> {
    pub binding: &'a Binding,
    pub target: &'a RouteTarget,
    pub request: TurnRequest,
    pub config_digest: &'a str,
}

impl Catalog {
    /// Explain admission using a caller-supplied conservative input-token bound for all candidates.
    /// Unknown counts remain unknown and cannot prove context admission. No tokenization is implied.
    /// # Errors
    /// Refuses invalid input and unknown aliases. Capability refusals remain in the explanation.
    pub fn explain(
        &self,
        request: &TurnRequest,
        input_tokens: Option<u64>,
    ) -> Result<RouteExplanation, Error> {
        request.validate()?;
        let alias = Id::new(&request.model).map_err(|_| Error::invalid("invalid route alias"))?;
        let route = self
            .routes
            .get(&alias)
            .ok_or_else(|| Error::invalid("unknown route alias"))?;
        let mut targets = Vec::with_capacity(route.targets.len());
        let mut selected = None;
        for target in &route.targets {
            let binding = self
                .bindings
                .get(&target.serving_model_id)
                .ok_or_else(|| Error::invalid("route binding is missing"))?;
            let mut rejections = capability_rejections(binding, request, input_tokens);
            if target.position > 0 && !route.declaration.fallback_enabled {
                rejections.push(Rejection::FallbackDisabled);
            }
            if rejections.is_empty() && selected.is_none() {
                selected = Some(target.id.clone());
            }
            targets.push(TargetExplanation {
                target_id: target.id.clone(),
                serving_model_id: target.serving_model_id.clone(),
                position: target.position,
                provenance: binding.provenance().clone(),
                auth_kind: binding.declaration().account.auth_kind,
                billing_kind: binding.declaration().account.billing_kind,
                capabilities: binding.capabilities().clone(),
                rejections,
            });
        }
        Ok(RouteExplanation {
            route_id: route.declaration.id.clone(),
            alias,
            config_digest: self.digest.clone(),
            input_tokens,
            selected_target_id: selected,
            targets,
        })
    }

    /// Select the first explicitly permitted compatible target, without making an inference call.
    /// # Errors
    /// Refuses malformed input, unknown aliases or all incompatible targets, naming their reasons.
    pub fn resolve(
        &self,
        request: &TurnRequest,
        input_tokens: Option<u64>,
    ) -> Result<Selection<'_>, Error> {
        let explanation = self.explain(request, input_tokens)?;
        let selected = explanation
            .selected_target_id
            .as_ref()
            .ok_or_else(|| crate::fallback::no_compatible_target(&explanation))?;
        self.select(&explanation.alias, selected, request)
    }

    /// Bind an explained, admissible target; the request keeps every setting but the model name.
    pub(crate) fn select(
        &self,
        alias: &Id,
        selected: &Id,
        request: &TurnRequest,
    ) -> Result<Selection<'_>, Error> {
        let target = self
            .routes
            .get(alias)
            .and_then(|route| route.targets.iter().find(|target| &target.id == selected))
            .ok_or_else(|| Error::invalid("selected route target is missing"))?;
        let binding = self
            .bindings
            .get(&target.serving_model_id)
            .ok_or_else(|| Error::invalid("selected binding is missing"))?;
        let mut request = request.clone();
        binding
            .provenance()
            .model
            .as_str()
            .clone_into(&mut request.model);
        request.validate_for(binding.provenance(), binding.capabilities())?;
        Ok(Selection {
            binding,
            target,
            request,
            config_digest: &self.digest,
        })
    }
}

fn capability_rejections(
    binding: &Binding,
    request: &TurnRequest,
    input_tokens: Option<u64>,
) -> Vec<Rejection> {
    let caps = binding.capabilities();
    let mut reasons = Vec::new();
    for (rejected, reason) in [
        (!request.tools.is_empty() && !caps.tools, Rejection::Tools),
        (
            !request.tool_choice.is_auto() && !caps.tool_choice,
            Rejection::ToolChoice,
        ),
        (
            request.sampling.temperature.is_some() && !caps.temperature,
            Rejection::Temperature,
        ),
        (
            request.sampling.top_p.is_some() && !caps.top_p,
            Rejection::TopP,
        ),
        (
            request
                .sampling
                .reasoning_effort
                .as_ref()
                .is_some_and(|effort| !caps.reasoning_efforts.contains(effort)),
            Rejection::ReasoningEffort,
        ),
        (
            request
                .max_output_tokens
                .is_some_and(|limit| limit > caps.max_output_tokens),
            Rejection::OutputLimit,
        ),
    ] {
        if rejected {
            reasons.push(reason);
        }
    }
    match input_tokens {
        None => reasons.push(Rejection::InputTokensUnknown),
        Some(input) => {
            let output = request.max_output_tokens.unwrap_or(caps.max_output_tokens);
            if input
                .checked_add(output)
                .is_none_or(|total| total > caps.context_window)
            {
                reasons.push(Rejection::ContextWindow);
            }
        }
    }
    if request.items.iter().any(|item| matches!(item, Item::Opaque { provenance, .. } if provenance != binding.provenance())) {
        reasons.push(Rejection::OpaqueState);
    }
    reasons
}
