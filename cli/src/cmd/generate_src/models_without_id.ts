type ΩIfEquals<T, U, Y = unknown, N = never> =
    (<G>() => G extends T ? 1 : 2) extends (<G>() => G extends U ? 1 : 2) ? Y
        : N;
