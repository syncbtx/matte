x :: 4;
y :: 3;

z :: x + y;

add x y :: x + y;

square x :: x * x;

max x y:: if x > y then x else y; -- max x y:: x > y ? x : y;

max 3 7;

res :: max x y;

square @@ max 3 7; -- square (max 3 7);

print res;

print typeof max;
