# RFC 3: Deploy

This RFC describes the deployment model of ChiselStrike runtime.

## Application configuration

Application is defined by a configuration file, which describes its types and endpoints:

```
app "paper-search" {
  types = [ "paper.graphql" ]
  endpoints = [ "paper.ts" ]
}

A new application skeleton can be created with the `chisel new` command.

## Local deployments

An application can be deployed locally with:


```
$ chisel run
```

The command assumes that a default file `chisel.cf` exists.

## Remote deployments

Remote deployments to ChiselStrike service first require the user to authenticate:

```
$ chisel login
```

The user can then see what transformations are needed to deploy the application to the serice:

```
$ chisel plan
```

Finally, the user can apply the required transformations to deploy the application to ChiselStrike service:

```
$ chisel apply
```
