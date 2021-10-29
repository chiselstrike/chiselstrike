# Getting Started

## Server startup

To get started, the first thing we need is a `chiseld` server running.

To start up the server, run:

```
chiseld
```

And now you have the ChiselStrike server listening to HTTP port 3000 on localhost.

Next, we will define some types in the ChiselStrike type system.

## Types 

Types are a way to define domain objects in your application.
The types are defined using GraphQL schemas and registered to ChiselStrike via the `chisel type import` command.

For example, to define a type `Person` with two `String` fields, `first_name` and `last_name`, run:

```
cat << EOF | chisel type import -
type Person {
  first_name: String
  last_name: String
}
EOF
```

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
curl --data '{"first_name":"Glauber", "last_name":"Costa"}' -o - localhost:3000/create-person
```

The next endpoint we will create is a `/find-all-people` endpoint that returns all objects of type `Person`:

```
cat << EOF | chisel end-point create find-all-people -
async function chisel(req) {
    let response = "";
    let people = await Chisel.find_all("Person");
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
curl -o - localhost:3000/find-all-people
```