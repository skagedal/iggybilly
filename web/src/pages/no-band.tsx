import { Layout } from "../components/Layout";
import type { NoBandProps } from "../types";

/**
 * The only screen a user in no band can reach.
 *
 * Signing in worked — the password was right — and there is simply
 * nowhere to go: a new account before the site owner has assigned it a
 * band, or someone whose band has been wound up. So this says what is
 * wrong and who can fix it, and offers the one action that still makes
 * sense. It is not an error page: nothing has gone wrong.
 */
export default function NoBandPage({ username }: NoBandProps) {
  return (
    <Layout username={username}>
      <section className="centered">
        <h1>You are not in a band yet</h1>
        <p>
          Your account works, but it has not been added to a band, so there
          are no clips for you to see. Ask whoever runs this iggybilly to
          add you to one.
        </p>
      </section>
    </Layout>
  );
}
