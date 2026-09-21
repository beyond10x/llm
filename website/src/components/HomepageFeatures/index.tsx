import type {ReactNode} from 'react';
import Link from '@docusaurus/Link';
import Heading from '@theme/Heading';
import styles from './styles.module.css';

type FeatureItem = {
  title: string;
  governs: string;
  question: string;
  description: ReactNode;
  href: string;
};

const FeatureList: FeatureItem[] = [
  {
    title: 'Describe',
    governs: 'one neutral turn',
    question: 'What did the caller ask for?',
    description: (
      <>
        A turn carries text, tool definitions, tool results and opaque provider items. Tool
        definitions confer no execution permission — LLM never runs a tool. Absent sampling fields
        stay absent, and unsupported settings are refused before any network I/O.
      </>
    ),
    href: '/docs/concepts/neutral-boundary',
  },
  {
    title: 'Select',
    governs: 'explained routing',
    question: 'Which binding may serve it, and why?',
    description: (
      <>
        A strict TOML catalog names providers, accounts, endpoints, served models and capabilities.
        Selection is ordered and opt-in, admission needs a caller-supplied input-token bound, and
        the explanation exposes ids and refusal reasons — never a secret or a prompt.
      </>
    ),
    href: '/docs/concepts/routing',
  },
  {
    title: 'Account',
    governs: 'attributable spend',
    question: 'What was actually observed, and what is unknown?',
    description: (
      <>
        Every usage field is independently optional. Exact decimal arithmetic separates reference
        valuations, metered and compute estimates, and recorded charges. A missing counter stays
        missing: unknown is never zero and never the configured value.
      </>
    ),
    href: '/docs/concepts/accounting',
  },
];

function Feature({title, governs, question, description, href}: FeatureItem) {
  return (
    <article className={styles.card}>
      <div className={styles.cardHeader}>
        <span className={styles.cardGoverns}>{governs}</span>
        <span className={styles.cardArrow} aria-hidden="true">→</span>
      </div>
      <Heading as="h3" className={styles.cardTitle}>
        <Link to={href} className={styles.cardLink}>{title}</Link>
      </Heading>
      <p className={styles.cardQuestion}>{question}</p>
      <p className={styles.cardBody}>{description}</p>
    </article>
  );
}

export default function HomepageFeatures(): ReactNode {
  return (
    <section className={styles.features}>
      <div className={styles.inner}>
        <div className={styles.header}>
          <div className={styles.eyebrow}>
            <span className={styles.ordinal}>02</span>
            <span>The implemented boundary</span>
          </div>
        </div>
        <Heading as="h2" className={styles.title}>Describe, select, account</Heading>
        <div className={styles.grid}>
          {FeatureList.map((props) => <Feature key={props.title} {...props} />)}
        </div>
      </div>
    </section>
  );
}
