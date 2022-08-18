# Streaming

In this section, we will show how to integrate your application with Kafka streaming.

## Consuming topics

To consume Kafka topics, you first need to write an `events/<topic>.ts` file that is the topic event handler function.

For example, to consume events from a `hello` topic, create a `events/hello.ts` file with the following contents:

```typescript title="my-backend/events/hello.ts"
import { ChiselEvent } from "@chiselstrike/api";

function toJSON(buffer: ArrayBuffer) {
    return JSON.parse(String.fromCharCode.apply(null, new Uint8Array(buffer)));
}

export default async function (event: ChiselEvent) {
    console.log(toJSON(event.value));
}
```

Then, pass the `--kafka-connection <host>` and `--kafka-topics <topic>` command line options to the ChiselStrike server.

For example, if your Kafka broker is running on `localhost:9092` and you want to subscribe to the `hello` topic, run:

```
npm run dev -- -- --kafka-connection localhost:9092 --kafka-topics hello
```

That's it! Now whenever there is an event on the `hello` topic, your event handler function is called.
