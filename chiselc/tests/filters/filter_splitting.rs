// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use super::*;

#[test]
fn side_effect() {
    let compiled = compile!(
        r#"Person.cursor().filter(person => person.age > 40 && fetch("foobar"))"#,
        "Person"
    );

    let expected = r#"
        Person.cursor().__filter((person)=>person.age > 40 , {
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
                value: 40
            }
        }, (person)=>fetch("foobar")
    );"#;
    assert_ast_eq!(compiled, expected);

    let compiled = compile!(
        r#"Person.cursor().filter(person => person.name == "Glauber Costa" && person.age > 40 && validate(person));"#,
        "Person"
    );
    let expected = r#"
        Person.cursor().__filter((person)=>person.name == "Glauber Costa" && person.age > 40
        , {
            exprType: "Binary",
            left: {
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
                    value: "Glauber Costa"
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
                op: "Gt",
                right: {
                    exprType: "Value",
                    value: 40
                }
            }
        }, (person)=>validate(person)
    );"#;

    assert_ast_eq!(compiled, expected);

    let compiled = compile!(
        r#"
         const age = 40;
         Person.cursor().filter(person => person.age > age && fetch("foobar"))
        "#,
        "Person"
    );
    let expected = r#"
        const age = 40;
        Person.cursor().__filter((person)=>person.age > age
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
            op: "Gt",
            right: {
                exprType: "Value",
                value: age
            }
        }, (person)=>fetch("foobar")
        );"#;
    assert_ast_eq!(compiled, expected);

    let compiled = compile!(
        r#"Person.cursor().filter(person => { return person.age > 40 && fetch("foobar"); })"#,
        "Person"
    );
    let expected = r#"
        Person.cursor().__filter((person)=>{
            return person.age > 40;
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
                value: 40
            }
        }, (person)=>{
            return fetch("foobar");
        });"#;
    assert_ast_eq!(compiled, expected);
}

#[test]
fn side_effects_unoptimizable() {
    assert_no_transform!(
        r#"Person.cursor().filter(person => person.age > 40 || fetch("foobar"))"#,
        "Person"
    );

    assert_no_transform!(
        r#"Person.cursor().filter(person => fetch("foobar") && person.age > 40)"#,
        "Person"
    );
}

#[test]
fn find_one() {
    let compiled = compile!(
        r#"Person.findOne(person => person.name == "Glauber Costa" && validate(person));"#,
        "Person"
    );

    let expected = r#"
        Person.__findOne((person)=>person.name == "Glauber Costa"
        , {
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
                value: "Glauber Costa"
            }

        }, (person)=>validate(person)
        );
        "#;

    assert_ast_eq!(compiled, expected);
}
