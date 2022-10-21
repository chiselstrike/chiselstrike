use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;
use crate::schema;

pub type Relation = (Arc<schema::Type>, Arc<schema::Type>);

pub fn evaluate_relation<StepF>(
    schema_a: &schema::Schema,
    schema_b: &schema::Schema,
    goal: Relation,
    mut step_f: StepF
) -> Result<()>
    where StepF: FnMut(Relation, &mut Vec<Relation>) -> Result<()>
{
    let mut assumptions: HashSet<Relation> = HashSet::new();
    let mut goals: Vec<Relation> = vec![goal];

    while let Some(goal) = goals.pop() {
        if !assumptions.insert(goal.clone()) {
            continue;
        }

        let (goal_a, goal_b) = goal;

        if let Some(goal_deref_a) = deref_typedef(schema_a, &goal_a) {
            goals.push((goal_deref_a, goal_b));
            continue;
        }
        if let Some(goal_deref_b) = deref_typedef(schema_b, &goal_b) {
            goals.push((goal_a, goal_deref_b));
            continue;
        }

        step_f((goal_a, goal_b), &mut goals)?;
    }

    Ok(())
}

fn deref_typedef(schema: &schema::Schema, type_: &schema::Type) -> Option<Arc<schema::Type>> {
    if let schema::Type::Typedef(type_name) = type_ {
        Some(schema.typedefs[type_name].clone())
    } else {
        None
    }
}
