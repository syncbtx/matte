twice f x :: f (f x);
add1 x :: x + 1;
print twice add1 10;

make_adder x :: fn y -> x + y;
add5 :: make_adder 5;
print add5 10;
