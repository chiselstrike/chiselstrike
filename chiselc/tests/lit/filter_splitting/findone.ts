// Test splitting a findOne filter with side-effects.
// RUN: @chiselc @file -e Person

Person.findOne(person => person.name == "Glauber Costa" && validate(person));

// CHECK: Person.__findOne((person)=>person.name == "Glauber Costa"
// CHECK: , {
// CHECK:     exprType: "Binary",
// CHECK:     left: {
// CHECK:         exprType: "Property",
// CHECK:         object: {
// CHECK:             exprType: "Parameter",
// CHECK:             position: 0
// CHECK:         },
// CHECK:         property: "name"
// CHECK:     },
// CHECK:     op: "Eq",
// CHECK:     right: {
// CHECK:         exprType: "Literal",
// CHECK:         value: "Glauber Costa"
// CHECK:     }
// CHECK: }, (person)=>validate(person)
// CHECK: );
