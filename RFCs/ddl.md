This is the v1 proposal for ChiselStrike data description language (DDL).
This is not yet the short term vision, so I am not using Prisma.

To ground the language in a real use case, I am the following use case, that
one of our prospects shared:

* You have users in Europe and the US. European data has to be stored in European
  datacenters, and US data will be stored in American datacenters.
* A user requests that their information is deleted from the system.
* A profile page for that user now shows a 404 error -- that is the easy part.
* But what if there was an invoice that mentioned that user? You can't delete the
  invoice, but shouldn't leak information either.


To solve this example problem, we have to define the regions where you want to make
your data available. We could do this in a file, regions.yaml

```
$ cat regions.yaml

eu: [ aws:eu-west-1 ]
us_CA: [ aws:us-west-1, aws:us-west-2 ]
```

We then create 3 types (GraphQL syntax), and annotate them with `@` directives

```
$ cat types.graphql

type User {
  uid @id
  name, @personal
  email, @email
  address, @personal
}

type Provider {
   company_id @id
   name,
}

type Invoices {
   id @id,
   customer User,
   provider Provider,
   date Date,
   description: Text,
   total: Currency,
   amount_paid: Currency, // total + taxes
}
```

Next, we will actually define the behavior we want ChiselStrike to uphold:

```
$ cat cstrk.yaml
label:
  name: "personal"
  availability: 10.0.0.0/16
  regions: [ "eu", "us_CA" ]

label:
   name: "email"
   # email is also a personal field
   parent: "personal"

# catch all for stuff that doesn't have labels
default:
  availability: "0.0.0.0"
  regions: "*"

endpoint:
  name: "invoice_*"
  # that means that when we authenticate, the user is assigned a provider_id. For example, if you were to authenticate from a @chiselstrike.com
  # address, your company would be ChiselStrike (which has an ID), and the user is now mapping this to provider_id
  auth_token: provider_id
  # what this is saying is that if provider_id != Invoice.Provider.company_id, we will deny it right away, unless you are a superadmin (0xdeadbeef)
  # Note that the interesting part here is not only that it has no-code auth, but that the policy maker and the endpoint behavioral description are
  # separate
  auth_token_match: Invoice.Provider.company_id || 0xdeadbeef
  encryption: no
  # mutate is update, delete, create
  provisioned_mutate_per_second: 1000
  provisioned_mutate_per_day: 200000
  # could be "best_case", or "throttle", or whatever else.
  provisioned_mutate_limit_action: 404
  # free to read, all modifications are audited.
  audit: mutate
  provisioned_read_per_second: 1000
  provisioned_read_per_day: 200000
  provisioned_read_limit_action: "best_effort"
  label_action:
        # biggest challenge is how to express those actions. In this case we want to allow to show a delete user, but show default values
	personal: if not User.id in Users { name : "Not found" }
        # no conditional, always hide the email, like g***@chiselstrike.com
        email: User.email.hide()
```

Now the task of describing the behavior of an endpoint should be simplified. More importantly, policies are applied regardless of how the frontend
personal writes the endpoints. And labels are leaky. So for example, consider an endpoint that wants to return the total between two dates. The end
result doesn't touch any personal field, so the `personal` policies are not applied, and this endpoint can be public:

```
endpoint(invoice_total_by_provider) {
  // params = provider_id, date_start, date_end
  sum := 0;

  for x in types.Invoices.filter(|x| x.provider.id == provider_id).filter(|x| params.date_start > x.date).filter(|x| params.date_end < x.date) {
     sum += x.amount_paid
  }

  return sum
}
```

But and endpoint that shows a particular invoice by its id, would show personal information.
Not only policies are applied (in this case they will be private), but the user's email would automatically be masked,
and users that are deleted would not show:

```
endpoint(invoice_by_id) {
  // params = invoice_id
  return types.Invoices[params.invoice_id]
}
```
