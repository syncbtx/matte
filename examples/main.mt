fib n :: if n <= 1 then n else fib (n - 1) + fib (n - 2);

print fib 10;
print fib 20;

fact n :: if n <= 1 then 1 else n * fact (n - 1);

print fact 5;
print fact 10;
