# ChiselStrike with Gatsby Example

This example implements a blog page where there are multiple posts made in
markdown and for each post anyone can comment and see other people's comments
through ChiselStrike.

## Instructions to run the example

First, we need to set up ChiselStrike (located at the `backend` directory) and
Gatsby to run the project. We can do both at the same time by just installing
the necessary packages at the root of the project:

```bash
npm install
```

And then to start Gatsby and ChiselStrike in development mode, run the command:

```bash
npm run start
```

This should start Gatsby and the ChiselStrike server at the same time. The blog
should be accessible in `localhost:8000`, selecting a post and creating a
comment should be possible, as well as seeing previous comments on each blog
post.

Chiselstrike is brought up together with Gatsby because of the
`gatsby-chisel` plugin. If this plugin is not used, then ChiselStrike needs to
be started manually on the `backend` directory with `npm run dev` in another terminal from
the one with Gatsby.

---

**NOTE**

`npm install` at the root of the project is going to install Gatsby packages
**and then** go into the `backend` directory and install ChiselStrike's
packages. This happens automatically because of the `postinstall` script on the
`package.json` that handles this. Without it, you would have to manually run
`npm install` twice, once in the root for Gatsby and another in the `backend`
for ChiselStrike

---

## How this example was made

This section details the step-by-step process that was used to create this
example. This serves more as a report of sorts, and it should be used for
understanding the process to create this example; Most importantly, this section can
later be used as a base for a future blog post for ChiselStrike with Gatsby.

### Step 0 - Gatsby Setup

For the Gatsby setup, we used a
[starter](https://www.gatsbyjs.com/starters/gatsbyjs/gatsby-starter-blog) on the
Gatsby site that is a boilerplate for blog sites. Then added Tailwind CSS to
make styling easier as well as getting a template for a comment section with
this [example](https://tailwindcomponents.com/component/comment-form).

The starter uses blogs written in Markdown as the source of content. While we
could save these blogs on the database, we felt that we should respect the ways
the Gatsby community deals with posts and just extend these Markdown posts with
comments that come from ChiselStrike. This way, Gatsby users can see that they
don't have to ditch their entire way of doing things up until now, ChiselStrike
would just be added on top of what is possible. So on unto adding comments to
these posts!

### Step 1 - ChiselStrike types

Here were are assuming that you already have the ChiselStrike working and
`chisel` is accessible with the path variables.

The first thing we want to do is to create the model (also called
types/entities) of the database. Because we only want `comments` to be dynamic
for this project, we only have to create a `BlogComment` model representing a
comment. We can do this by initiating ChiselStrike on a directory with
`npx create-chiselstrike-app backend` and then create the model on the
`backend/models` directory like so:

```typescript
class BlogComment {
  postId: string // The Gatsby's id for the post
  content: string // The comment's text
  postedAt: string // When the post was posted in ISO string format
}
```

In this case, we create a `postId` so that we can associate a `BlogComment` with
the id that Gatsby assigns to a post through the markdown local API. As for
`postedAt` being a string is due to the `Date` type not being supported by
ChiselStrike at the moment, so it holds a string with the ISO format for the
date, which is later going to be parsed by Javascript for date operations.

### Step 2 - ChiselStrike endpoints

For this example, we are going to need one endpoint with two methods: one for
the `GET` method that will either get all comments (for testing purposes) or all
comments for a specified post, and one for the `POST` method that will create a
new comment for a specific post.

The endpoint for this is just like the one on `backend/endpoints/comments.ts`.
Notice that for the `GET` method, if no `postId` is given as a query parameter,
then the endpoint will return all comments of the database (this can be
dangerous if no pagination is used), otherwise the comments fetched will
be only those for a specific post with `postId`.

For the `POST` method that will create a new post, it just calls the `save`
method from `BlogComment` using the body given by the request, while also
augmenting it by including a timestamp for the creation date.

With these endpoints, we can now do the frontend code to implement comments on
our blog!

### Step 3 - Calling ChiselStrike on the frontend

First we need a `Comment` component that is going to be used to represent a
comment. This can be found on `src/components/Comment`, with the whole comment
section being found on `src/components/comment-sections.js`.

Gatsby has many way to get data from sources and render a page, such as: static
site generation (SSG), server side rendering (SSR) and client side rendering.
Because comments are dynamic data, we can't use SSG, otherwise they would be
become outdated pretty easily and we would have to build the project again to
get the new comments every time.

While SSR seems good on paper for this, comments are a non-essential part of the
page in which the most essential part (the blog post) already was rendered in
SSG (which is fast). If we were to put SSR, the SSG used for the blog post is
going to be useless because the page request would now have to go to the server,
the server would get the comments, it would build the page and then deliver the
built page to the user, totally ignoring the fact that the component for the
post was already built. SSR would make the delivery of the essential part of the
page slow because it would deliver the post + comments at the same time.

What if we had a way to deliver the SSG post as fast as possible **and then** do
the request for comments and render the comments section when that's ready? That
is what client side render does! So for this example, we are going to treat the
component as a normal React component and get the data when the component
javascript's loads up, as well as when a new comment is created by the user.

To do this we extended the `src/templates/blog-post.js` that came with the
starter, adding a comment section and logic to get and create comments through a
Axios request to ChiselStrike. Some notable points are:

- `getCommentsFromChisel` is the function used to get all comments from a post
  from ChiselStrike and format them.
- `handleCommentCreation` is the function that is going to be used to create a new
  comment, as well as update the comments after that is done.
- `useEffect` this is going to get the comments the first time when the blog
  post renders. This will be run only after Javascript is loaded into the page,
  so the post gets delivered first and then the comments are fetched.
