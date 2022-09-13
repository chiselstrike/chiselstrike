// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use super::*;

#[test]
fn invalid_transform() {
    assert_no_transform!(r#"Person.cursor().filter(() => true);"#, "Person");
    assert_no_transform!(
        r#"Person.cursor().filter((person) => true, foo);"#,
        "Person"
    );
    assert_no_transform!(
        r#"
        Person.cursor().filter((person) => {
            console.log("hello!");
            return true;
        });
        "#,
        "Person"
    );
}

#[test]
fn transform_filter_in_main() {
    let compiled = compile!(
        r#"
        class Person extends Model {
          id: number;
          name: string;
          age: number;
        }

        const main = async () => {
          const people = await Person.cursor()
            .filter((p) => {
              return p.age > 4
            }).toArray();
        };
        "#,
        "Person"
    );

    let expected = r#"
        class Person extends Model {
          id: number;
          name: string;
          age: number;
        }

        const main = async ()=>{
            const people = await Person.cursor().__filter((p)=>{
                return p.age > 4;
            }, {
                exprType: "Binary",
                left: {
                    exprType: "Property",
                    object: {
                        exprType: "Parameter",
                        position: 0
                    },
                    property: "age"
                },
                op: "Gt",
                right: {
                    exprType: "Value",
                    value: 4
                }
            }).toArray();
        };"#;

    assert_ast_eq!(compiled, expected);
}

#[test]
fn bind_simple_filter_expression() {
    let compiled = compile!(
        r#"
        const people = await Person.cursor()
          .filter((p) => {
            return p.age > 4
          }).toArray();
        "#,
        "Person"
    );

    let expected = r#"
        const people = await Person.cursor().__filter((p)=>{
            return p.age > 4;
        }, {
            exprType: "Binary",
            left: {
                exprType: "Property",
                object: {
                    exprType: "Parameter",
                    position: 0
                },
                property: "age"
            },
            op: "Gt",
            right: {
                exprType: "Value",
                value: 4
            }
        }).toArray();"#;

    assert_ast_eq!(compiled, expected);
}

#[test]
// The lambda has an expression statement so we should not transform it.
fn filter_expression_statement() {
    let compiled = compile!(
        r#"
        const people = await Person.cursor()
          .filter((p) => {
            p.age > 4;
          }).toArray();
        "#,
        "Person"
    );

    let expected = r#"
        const people = await Person.cursor().filter((p)=>{
            p.age > 4;
        }).toArray();
    "#;

    assert_ast_eq!(compiled, expected);
}

#[test]
fn simple_transform_return_statement() {
    let compiled = compile!(
        r#"await Person.cursor().filter((p) => { return p.age < 4 });"#,
        "Person"
    );

    let expected = r#"
        await Person.cursor().__filter((p)=>{
            return p.age < 4;
        }, {
           exprType: "Binary",
           left: {
               exprType: "Property",
               object: {
                   exprType: "Parameter",
                   position: 0
               },
               property: "age"
           },
           op: "Lt",
           right: {
               exprType: "Value",
               value: 4
           }
        });"#;
    assert_ast_eq!(compiled, expected);

    let compiled = compile!(
        r#"await Person.cursor().filter((p) => { return p.age > 4 });"#,
        "Person"
    );

    let expected = r#"
        await Person.cursor().__filter((p)=>{
            return p.age > 4;
        }, {
            exprType: "Binary",
            left: {
                exprType: "Property",
                object: {
                    exprType: "Parameter",
                    position: 0
                },
                property: "age"
            },
            op: "Gt",
            right: {
                exprType: "Value",
                value: 4
            }
        });
        "#;
    assert_ast_eq!(compiled, expected);

    let compiled = compile!(
        r#"await Person.cursor().filter((p) => { return p.age <= 4 });"#,
        "Person"
    );

    let expected = r#"
        await Person.cursor().__filter((p)=>{
            return p.age <= 4;
        }, {
            exprType: "Binary",
            left: {
                exprType: "Property",
                object: {
                    exprType: "Parameter",
                    position: 0
                },
                property: "age"
            },
            op: "LtEq",
            right: {
                exprType: "Value",
                value: 4
            }
        });
        "#;
    assert_ast_eq!(compiled, expected);

    let compiled = compile!(
        r#"await Person.cursor().filter((p) => { return p.age != 4 });"#,
        "Person"
    );

    let expected = r#"
        await Person.cursor().__filter((p)=>{
            return p.age != 4;
        }, {
            exprType: "Binary",
            left: {
                exprType: "Property",
                object: {
                    exprType: "Parameter",
                    position: 0
                },
                property: "age"
            },
            op: "NotEq",
            right: {
                exprType: "Value",
                value: 4
            }
        });
        "#;
    assert_ast_eq!(compiled, expected);

    let compiled = compile!(
        r#"await Person.cursor().filter((p) => { return p.name == 'Alice' })"#,
        "Person"
    );

    let expected = r#"
        await Person.cursor().__filter((p)=>{
            return p.name == 'Alice';
        }, {
            exprType: "Binary",
            left: {
                exprType: "Property",
                object: {
                    exprType: "Parameter",
                    position: 0
                },
                property: "name"
            },
            op: "Eq",
            right: {
                exprType: "Value",
                value: "Alice"
            }
        });
        "#;
    assert_ast_eq!(compiled, expected);
}

#[test]
fn simple_filter_expression() {
    let compiled = compile!(
        r#"await Person.cursor().filter((p) => p.age < 4);"#,
        "Person"
    );

    let expected = r#"
        await Person.cursor().__filter((p)=>p.age < 4
        , {
            exprType: "Binary",
            left: {
                exprType: "Property",
                object: {
                    exprType: "Parameter",
                    position: 0
                },
                property: "age"
            },
            op: "Lt",
            right: {
                exprType: "Value",
                value: 4
            }
        });"#;
    assert_ast_eq!(compiled, expected);
}

#[test]
fn complex_return_statement() {
    let compiled = compile!(
        r#"await Person.cursor().filter((p) => { return p.age < 4 || (p.age > 10 && p.age != 12) });"#,
        "Person"
    );

    let expected = r#"
        await Person.cursor().__filter((p)=>{
            return p.age < 4 || (p.age > 10 && p.age != 12);
        }, {
            exprType: "Binary",
            left: {
                exprType: "Binary",
                left: {
                    exprType: "Property",
                    object: {
                        exprType: "Parameter",
                        position: 0
                    },
                    property: "age"
                },
                op: "Lt",
                right: {
                    exprType: "Value",
                    value: 4
                }
            },
            op: "Or",
            right: {
                exprType: "Binary",
                left: {
                    exprType: "Binary",
                    left: {
                        exprType: "Property",
                        object: {
                            exprType: "Parameter",
                            position: 0
                        },
                        property: "age"
                    },
                    op: "Gt",
                    right: {
                        exprType: "Value",
                        value: 10
                    }
                },
                op: "And",
                right: {
                    exprType: "Binary",
                    left: {
                        exprType: "Property",
                        object: {
                            exprType: "Parameter",
                            position: 0
                        },
                        property: "age"
                    },
                    op: "NotEq",
                    right: {
                        exprType: "Value",
                        value: 12
                    }
                }
            }
        });"#;
    assert_ast_eq!(compiled, expected);
}

#[test]
fn function_call() {
    assert_no_transform!(
        r#"
        function foo(p: Person) {
          return p.age < 4
        }
        await Person.cursor().filter((p) => { return foo(p) || (p.age > 10 && p.age != 12) })
        "#,
        "Person"
    );
}

#[test]
fn transform_filter_constant_expression() {
    let compiled = compile!(
        r#"await Person.cursor().filter(p => { return true; });"#,
        "Person"
    );

    let expected = r#"
        await Person.cursor().__filter((p)=>{
            return true;
        }, {
            exprType: "Value",
            value: true
        });"#;
    assert_ast_eq!(compiled, expected);

    let compiled = compile!(r#"await Person.cursor().filter(p => true);"#, "Person");

    let expected = r#"
        await Person.cursor().__filter((p)=>true
        , {
            exprType: "Value",
            value: true
        });"#;
    assert_ast_eq!(compiled, expected);
}

#[test]
fn empty_filter() {
    let compiled = compile!(r#"await Person.cursor().filter({});"#, "Person");

    let expected = r#"await Person.cursor().filter({})"#;
    assert_ast_eq!(compiled, expected);
}

#[test]
fn return_expression() {
    let compiled = compile!(
        r#"
        const name = "Pekka";
        await Person.cursor().filter((p) => {  return p.name == name; }).toArray();
        "#,
        "Person"
    );

    let expected = r#"
        const name = "Pekka";
        await Person.cursor().__filter((p)=>{
            return p.name == name;
        }, {
            exprType: "Binary",
            left: {
                exprType: "Property",
                object: {
                    exprType: "Parameter",
                    position: 0
                },
                property: "name"
            },
            op: "Eq",
            right: {
                exprType: "Value",
                value: name
            }
        }).toArray();"#;
    assert_ast_eq!(compiled, expected);
}
