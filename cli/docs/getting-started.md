# Getting Started

## Creating a project

To create a new ChiselStrike project, first create a directory:

```console
% mkdir -p hello && cd hello
```

Then run the `chisel init` command:

```console
% chisel init
Initialized ChiselStrike project in hello-world
```

## Starting the server

Once you have a project set up, the next step is to start the ChiselStrike
server in development mode with the `chisel dev` command:

```console
% chisel dev
INFO - ChiselStrike is ready 🚀 - URL: http://127.0.0.1:8080
End point defined: /dev/hello
```

The ChiselStrike server is now listening to URL `http://127.0.0.1:8080` with an
endpoint mounted at `/dev/hello`.

You can access the endpoint with `curl`, for example:

```
% curl http://127.0.0.1:8080/dev/hello
"hello, world!"%
```

## Endpoints

Endpoints are a way to define your application business logic with TypeScript.

To define a new endpoint, you need to create a file with your endpoint code in
the `endpoints` directory.

For example, create the file `endpoints/endpoint.ts` with the following
contents:

```typescript
export default async function chisel(req) {
    return new Response('hello, endpoint');
}
```

If you have `chisel dev` running in the background, the new endpoint is picked
up automatically, and you can access it at
`http://127.0.0.1:8080/dev/endpoint`. If you are not using `chisel dev`, you
need to run the `chisel apply` command for the new endpoint to become
visible.

## Types 

Types are a way to define domain objects in your application.
The types are defined using GraphQL schemas and registered to ChiselStrike via the `chisel apply` command.

For example, to define a type `Person` with two `String` fields,
`first_name` and `last_name`, create a file in the `types` directory with

```
type Person {
  first_name: String
  last_name: String
}
```

and run `chisel apply`.

The command will output the following:

```
Type defined: Person
```

And now you have the type `Person` defined in the ChiselStrike type system.
The type can be now accessed and persisted via the `Chisel` module in your endpoints.

## Endpoints

Now that we have a type defined, the next step is to define an endpoint that uses it.

The first endpoint we will create is a `/create-person` endpoint that accepts JSON as HTTP POST and persists the JSON as a `Person` type.

To do that, run:

```
cat << EOF | chisel end-point create create-person -
async function chisel(req) {
    if (req.method == 'POST') {
        const payload = await req.json();
        await Chisel.store('Person', payload);
        return new Response('ok\n');
    }
    return new Response('ignored\n');
}
EOF
```

The command ouputs the following:

```
End point defined: /create-person
```

and now you can create a new `Person` object with:

```
curl --data '{"first_name":"Glauber", "last_name":"Costa"}' -o - localhost:8080/create-person
```

The next endpoint we will create is a `/find-all-people` endpoint that returns all objects of type `Person`:

```
cat << EOF | chisel end-point create find-all-people -
async function chisel(req) {
    let response = "";
    let people = await Chisel.collections.Person.rows();
    for await (let person of people) {
        response += person.first_name + " " + person.last_name;
        response += " ";
    }
    return new Response(response);
}
EOF
```

The command outputs:

```
End point defined: /find-all-people
```

and now you can query all the objects of type `Person` in the system with:

```
curl -o - localhost:8080/find-all-people
```
