# ChiselStrike docs

These docs are built using [Docusaurus 2](https://docusaurus.io/), a modern static website generator.

## Adding more docs

Just add the Markdown in the `./docs` folder.

It's important to add `slug: /` to the top of the markdown file that you want to use as home. Right now is `./docs/intro`


### Installation

```
$ yarn
```

### Local Development

```
$ yarn start
```

This command starts a local development server and opens up a browser window. Most changes are reflected live without having to restart the server.

### Build

```
$ yarn build
```

This command generates static content into the `build` directory and can be served using any static contents hosting service.

### Deployment

Using SSH:

```
$ USE_SSH=true yarn deploy
```

Not using SSH:

```
$ GIT_USER=<Your GitHub username> yarn deploy
```

If you are using GitHub pages for hosting, this command is a convenient way to build the website and push to the `gh-pages` branch.
