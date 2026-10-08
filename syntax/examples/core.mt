x :: 4;
y :: 3;

z :: x + y;

add :: fn x -> fn y -> x + y;

square :: fn x -> x * x;

max :: fn x -> fn y -> if x > y then x else y;

max 3 7;

res :: max x y;

square (max 3 7);

print res;

print typeof max;
