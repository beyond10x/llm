import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import CodeBlock from '@theme/CodeBlock';
import Heading from '@theme/Heading';
import HomepageFeatures from '@site/src/components/HomepageFeatures';

import styles from './index.module.css';

const CATALOG = `format = "llm.catalog/1"

[[accounts]]
id = "remote"
provider_id = "my-lab"
auth_kind = "bearer"
billing_kind = "metered"
secret_reference_id = "lab-llm-token"

[[routes]]
id = "coding"
alias = "code"
# Omission is false. Set true to permit selection from the explicitly named alternatives.
fallback_enabled = false`;

const EXPLAINED = `{
  "target_id": "coding-secondary",
  "serving_model_id": "remote-large",
  "position": 1,
  "provenance": {
    "protocol": "responses",
    "provider": "my-lab",
    "account": "remote",
    "endpoint": "remote-models",
    "model": "large",
    "binding_revision": "5dd662a7d1b9e5280a10e6b691ff04c79326d1562d18d3206e9d1bd141ed9a13"
  },
  "auth_kind": "bearer",
  "billing_kind": "metered",
  "capabilities": {
    "tools": true,
    "tool_choice": true,
    "temperature": true,
    "top_p": true,
    "reasoning_efforts": [
      "medium",
      "high"
    ],
    "context_window": 32768,
    "max_output_tokens": 8192
  },
  "rejections": [
    "fallback-disabled"
  ]
}`;

type PanelProps = {
  ordinal: string;
  label: string;
  title: string;
  alt?: boolean;
  children: ReactNode;
};

function PanelSection({ordinal, label, title, alt, children}: PanelProps) {
  return (
    <section className={clsx(styles.section, alt && styles.sectionAlt)}>
      <div className={styles.sectionInner}>
        <div className={styles.panel}>
          <div className={styles.panelHeader}>
            <div className={styles.panelEyebrow}>
              <span className={styles.panelOrdinal}>{ordinal}</span>
              <span>{label}</span>
            </div>
          </div>
          <Heading as="h2" className={styles.panelTitle}>
            {title}
          </Heading>
          <div className={styles.panelBody}>{children}</div>
        </div>
      </div>
    </section>
  );
}

function HomepageHeader() {
  const {siteConfig} = useDocusaurusContext();
  return (
    <header className={clsx('hero', styles.heroBanner)}>
      <div className={styles.heroInner}>
        <Heading as="h1" className="hero__title">
          {siteConfig.title}
        </Heading>
        <p className="hero__subtitle">{siteConfig.tagline}</p>
        <div className={styles.buttons}>
          <Link className="button button--primary button--lg" to="/docs/getting-started">
            Run a turn locally
          </Link>
          <Link className="button button--secondary button--lg" to="/docs/status/where-this-stands">
            See what works today
          </Link>
        </div>
      </div>
    </header>
  );
}

function TheProblem() {
  return (
    <PanelSection
      ordinal="01"
      label="The problem"
      title="An inference client that guesses is worse than one that refuses">
      <p>
        Model clients grown inside an agent loop tend to acquire habits that cost money and hide
        facts: a missing token counter becomes a zero, a configured model name is substituted for
        the one the provider actually served, a transport failure after dispatch is retried as
        though it were free, and a subscription credential quietly falls back to a billable API key.
      </p>
      <CodeBlock language="text">
        {`usage.output_tokens = None   →  billed as 0
outcome.model         = "gpt-5"  (configured, never observed)
POST failed mid-stream →  retried; the first attempt may still bill`}
      </CodeBlock>
      <p>
        This repository exists to make each of those a refusal or an explicit unknown. Core
        validates structure and declared capabilities before any network I/O; a failure after
        dispatch is recorded as <code>unknown</code>, not as proof of a free retry; and an
        observation that the provider did not supply is absent rather than filled in.
      </p>
    </PanelSection>
  );
}

function TheClaim() {
  return (
    <PanelSection
      ordinal="03"
      label="The result"
      title="Selection you can read before a request is ever sent"
      alt>
      <p>
        On the left are two declarations from a strict <code>llm.catalog/1</code> TOML file. On
        the right is one element, verbatim, of what{' '}
        <code>cargo run -p b10x-llm-routing --example explain</code> prints for that catalog: the
        candidate it refused, and why. Explanation resolves no secret, provisions no resource and
        performs no I/O.
      </p>
      <div className={styles.compare}>
        <div className={styles.compareSide}>
          <CodeBlock language="toml" title="examples/catalog.toml">
            {CATALOG}
          </CodeBlock>
        </div>
        <div className={styles.compareArrow} aria-hidden="true">
          <span>→</span>
        </div>
        <div className={styles.compareSide}>
          <CodeBlock language="json" title="…the refused candidate, from the explanation">
            {EXPLAINED}
          </CodeBlock>
        </div>
      </div>
      <p className={styles.panelMore}>
        <Link to="/docs/guides/explain-a-route">Walk through the routing example →</Link>
      </p>
    </PanelSection>
  );
}

function HonestStatus() {
  return (
    <PanelSection
      ordinal="04"
      label="Status"
      title="Eleven of fourteen crates implemented; none of it qualified, none of it released">
      <div className={styles.ledger}>
        <p className={styles.ledgerBuilt}>
          Implemented and tested: the neutral core (asynchronous turns, tools, bounded streaming,
          cancellation, typed failures, target-bound opaque state); injected credential resolution
          with coordinated renewal and optional local file and keychain adapters; bounded
          single-attempt HTTP and SSE transport; validated provider/account/auth/protocol bindings;
          strict TOML catalog routing with safe explanation; exact usage pricing with a durable
          single-owner SQLite spending ledger; the Responses, Messages and Chat Completions
          projections; an authenticated single-owner gateway; and the hosting lifecycle contract.
        </p>
        <p className={styles.ledgerNot}>
          Not implemented: the operator command line, both cloud hosting adapters (Runpod and
          Modal), protocol translation in the gateway, and ordered runtime fallback. And the
          limit that bounds everything above it — <strong>no live provider credential has been
          used anywhere in this repository</strong>. Every scenario runs against fixtures, local
          sockets and in-process fakes, so no OpenAI or Anthropic access is qualified, no GPU has
          been allocated or stopped, and there is no release and no published artifact. Local
          fixture evidence is not provider qualification.
        </p>
      </div>
      <p className={styles.panelMore}>
        <Link to="/docs/status/where-this-stands">Story-by-story status →</Link>
        {' · '}
        <Link to="/docs/status/limitations">Limitations and trust boundary →</Link>
      </p>
    </PanelSection>
  );
}

export default function Home(): ReactNode {
  const {siteConfig} = useDocusaurusContext();
  return (
    <Layout title="Composable model inference" description={siteConfig.tagline as string}>
      <HomepageHeader />
      <main>
        <TheProblem />
        <HomepageFeatures />
        <TheClaim />
        <HonestStatus />
      </main>
    </Layout>
  );
}
