import { MinLength, MaxLength, is } from 'https://esm.sh/@deepkit/type?target=deno'

export default function chisel(req: Request) {
        type Goldilocks = string & MinLength<5> & MaxLength<10>;
        const ret = [is<Goldilocks>('1234'),
                     is<Goldilocks>('12345'),
                     is<Goldilocks>('123456789A'),
                     is<Goldilocks>('123456789AB')];
        return new Response(JSON.stringify(ret));
}
