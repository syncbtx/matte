-- ---------------------------------------------------------
-- Matte Demo Program
-- Showcasing recursion, currying, and church-encoded data
-- ---------------------------------------------------------

-- 1. Simple Recursion: Factorial
fact n :: if n <= 1 then 1 else n * fact (n - 1);

print fact 5;
print fact 10;

-- 2. Simple Recursion: Fibonacci
fib n :: if n <= 1 then n else fib (n - 1) + fib (n - 2);

print fib 10;

-- 3. Higher-order functions & Currying
make_adder x :: fn y -> x + y;
add5 :: make_adder 5;
add10 :: make_adder 10;

print add5 20;
print add10 50;

twice f x :: f (f x);
print twice add5 100;

-- 4. Church-encoded Pairs (Tuples)
-- A pair is a function that takes a selector 'f' and applies it to 'x' and 'y'
pair x y :: fn f -> f x y;
fst p :: p (fn x y -> x);
snd p :: p (fn x y -> y);

my_pair :: pair 42 99;
print fst my_pair;
print snd my_pair;

-- 5. Church-encoded Lists
-- A list is represented by its right-fold operation
nil :: fn f x -> x;
cons h t :: fn f x -> f h (t f x);

-- Create a list: [10, 20, 30]
my_list :: cons 10 (cons 20 (cons 30 nil));

-- Fold functions
sum_folder h acc :: h + acc;
prod_folder h acc :: h * acc;

sum lst :: lst sum_folder 0;
product lst :: lst prod_folder 1;

print sum my_list;
print product my_list;
