add :: fn x -> fn y -> x + y;
inc :: add 1;
twice :: fn f -> fn x -> f (f x);
make :: fn x -> twice (fn y -> x * y);
k :: fn x -> add x;
twice inc 0;
k 1 2;
deep :: fn a -> twice (fn b -> twice (fn c -> a + c) b);
(fn x -> x + 1) 2;
