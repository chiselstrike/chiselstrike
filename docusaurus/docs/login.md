---
sidebar_position: 5
---
# OAuth and Login

ChiselStrike supports the process of having your own users.  Your
website can let them log in using an OAuth provider, and your
ChiselStrike backend will be aware of who logged in and even maintain
a collection of all known users.  Let's examine how this works in
ChiselStrike.

## The Frontend Client

There are a couple of things the frontend needs when implementing the
user-login functionality:
1. A link the user can click to complete the OAuth procedure.
2. A way to tell if the user has already completed the OAuth
   procedure.
3. The logged-in user's information (eg, to display the profile page)
4. A way to inform the ChiselStrike backend we're acting on behalf of
   a logged-in user.

To provide these things in a handy way, we have developed a special
ChiselStrike library for use in frontends.  Here is how to use it.

:::note

This library will be provided to you as a part of the beta program.
It currently requires a session object for initialization, which
typically means this initialization must run inside a web server.  But
after initialization, its functions can be run either server-side or
browser-side.

We currently only support login via GitHub, using a test OAuth app
named `ChiselStrike Beta`.  In the future, we will support multiple
OAuth providers with client-specific OAuth identities.

:::

### Obtain a Client Instance

You can obtain a client instance by calling the library function
`getChiselStrikeClient`.  The client instance contains all the
internal info required for the other functions to work.

`getChiselStrikeClient` takes two arguments.  The first is a session
object -- something that persists its properties and provides a `save`
method.  A good example of this can be found in the [iron-session
package](https://github.com/vvo/iron-session/blob/0ac0b1b431783c28fbae86697239df55d461bc12/src/index.ts#L76).
The second argument is an object containing the URL parameters of the
ongoing web request as its properties.  For example, here is how
`getChiselStrikeClient` can be invoked in NextJS server-side
rendering:

```javascript
export const getServerSideProps = withIronSessionSsr(
    async function getServerSideProps(context) {
        const chisel = await getChiselStrikeClient(context.req.session, context.query);
        return { props: { chisel } };
    },
    sessionOptions
);
```

:::note

The library currently assumes that you're running your frontend server
on `localhost:3000` and that there is a valid page at
`http://localhost:3000/profile`.

:::

### Login/Logout

The client provides the link text in its property named `loginLink`.
For example:

```html
<p>Click <a href={chisel.loginLink}>here</a> to log in.</p>
```

When the login was successful, the ChiselStrike client object will
have a non-null `user` property that contains the user's name as a
string.

To log a user out, you must clear out the session object.  In
iron-session, for example, you would call `session.destroy()`.

### Accessing ChiselStrike Endpoints While Logged In

To make ChiselStrike recognize your logged-in user, the library
provides a function `chiselFetch` whose first argument is the
ChiselStrike client, while the rest of the arguments are the same as
in standard JavaScript
[fetch](https://developer.mozilla.org/en-US/docs/Web/API/fetch).  For
example:

```javascript
await chiselFetch(chisel, 'api/dev/comments, {
    method: 'GET',
})
```

When an endpoint is invoked like this, the backend will know the
identity of your logged-in user.  (And if your user logged out, this
call is identical to calling `fetch` directly.)
