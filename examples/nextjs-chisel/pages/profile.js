import Link from 'next/link'
import { getChiselStrikeClient } from "../lib/chiselstrike";
import { withSessionSsr } from "../lib/withSession";

export const getServerSideProps = withSessionSsr(
    async function getServerSideProps(context) {
        const chisel = await getChiselStrikeClient(context.req.session, context.query);
        return { props: { user: chisel.user, link: chisel.loginLink } };
    },
);

export default function Profile({ user, link }) {
    if (user)
        return <p>Profile for user {user}.  <Link href='/api/logout'>Log out</Link></p>;
    else {
        return <p>We don't know you.  Please <Link href={link}>log in</Link>.</p>
    }
}
