pair x y :: fn f -> f x y;
fst p :: p (fn x y -> x);
snd p :: p (fn x y -> y);

my_pair :: pair 10 20;
print fst my_pair;
print snd my_pair;
