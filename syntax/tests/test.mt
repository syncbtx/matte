add :: fn x -> fn y -> x + y;
twice :: fn f -> fn x -> f (f x);
inc :: add 1;
print typeof add;
print typeof inc;
print typeof twice;
twice inc 0;
