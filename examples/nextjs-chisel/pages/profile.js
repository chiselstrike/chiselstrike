import Link from 'next/link'

export async function getServerSideProps(context) {
    const user = context.query.user || null;
    return {
        props: { user },
    }
}

export default function Profile({ user }) {
    if (user)
        return <p>Profile for user {user}</p>;
    else {
        const link = `https://github.com/login/oauth/authorize?scope=read:user&client_id=${process.env.NEXT_PUBLIC_GHCLID}`;
        return <p>We don't know you.  Please <Link href={link}>log in</Link>.</p>
    }
}
