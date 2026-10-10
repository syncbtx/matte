nil :: fn f x -> x;
cons h t :: fn f x -> f h (t f x);

sum_folder h acc :: h + acc;
sum lst :: lst sum_folder 0;

my_list :: cons 10 (cons 20 (cons 30 (cons 40 nil)));
print sum my_list;
