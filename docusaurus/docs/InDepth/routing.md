# Routing

Like [Gatsby](https://www.gatsbyjs.com/docs/reference/routing/creating-routes/#define-routes-in-srcpages) and
[NextJS](https://nextjs.org/docs/routing/introduction#nested-routes), ChiselStrike routes incoming requests by
matching the URL path against the files in the `routes/` directory.

When you create a file `routes/posts.ts`, the URL
`/dev/posts` invokes it.  When you create a file `routes/new/york/city.ts`, the URL `/dev/new/york/city`
invokes it.

When there is no exact match, ChiselStrike uses the longest
prefix of the URL path that matches an existing route definition. In the previous example, the URL
`/dev/new/york/city/manhattan/downtown` will also be handled by `routes/new/york/city.ts` (assuming no
other routes).

This routing procedure enables request handlers to handle both the 'plural' and 'single' versions of themselves. 

For example, the above file `my-backend/routes/comments.ts` will be invoked when you access a specific comment, e.g., at
`/dev/comments/1234-abcd-5678-efgh`.  The `BlogComment.crud()` will parse the URL and understand that a single
collection element is being accessed.

