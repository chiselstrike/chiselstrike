// this trick is necessary, because `sqlx`-s use of lifetimes (and traits!) is just insane
// (apparently, by design, to avoid "misuse"):
//
// https://github.com/launchbadge/sqlx/issues/1428
// https://github.com/launchbadge/sqlx/issues/1594
pub unsafe fn reduce_args_lifetime<'q>(args: sqlx::any::AnyArguments<'static>) -> sqlx::any::AnyArguments<'q> {
    std::mem::transmute(args)
}
