import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import Heading from '@theme/Heading';

import styles from './styles.module.css';

/**
 * The 404, as a panel like every other panel on this site.
 *
 * Docusaurus ships a centred "Page Not Found" with a paragraph asking the reader to contact
 * whoever linked them. That is the wrong advice here: `onBrokenLinks` is `throw` in
 * `docusaurus.config.ts`, so a link written on this site cannot reach this page — what reached it
 * is an old URL or an outside one. The page says that, and then offers the four doors.
 */
export default function NotFoundContent({className}: {className?: string}): ReactNode {
  return (
    <main className={clsx(styles.wrapper, className)}>
      <div className={styles.panel}>
        <div className={styles.header}>
          <span className={styles.status}>404</span>
          <span>No such page</span>
        </div>
        <Heading as="h1" className={styles.title}>
          This address does not resolve to a page
        </Heading>
        <p className={styles.body}>
          Internal links are checked when the site is built, so this URL was not written here: it is
          either an address this site used to serve, or one that came from somewhere else.
        </p>
        <ul className={styles.links}>
          <li>
            <Link to="/docs">Documentation</Link>
            <span>what this is, and how to run it</span>
          </li>
          <li>
            <Link to="/docs/getting-started">Getting started</Link>
            <span>build the workspace and run the examples</span>
          </li>
          <li>
            <Link to="/docs/guides/explain-a-route">See it work</Link>
            <span>a catalog, and the selection it explains</span>
          </li>
          <li>
            <Link to="/docs/status/where-this-stands">Status</Link>
            <span>what is built, and what is not</span>
          </li>
        </ul>
      </div>
    </main>
  );
}
