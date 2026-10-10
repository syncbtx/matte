square x :: x * x;
twice :: fn f -> fn y -> f (f y);
print (twice square) 4;

fact n :: if n == 0 then 1 else n * fact (n-1);
print fact 7;
print typeof square;
