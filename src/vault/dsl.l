%%
^[-]?[0-9]+ "INT"
^[-]?[0-9]+[.][0-9]+$ "FLOAT"

\+ "+"
\* "*"
\- "-"
/ "/"
% "%"
> ">"
>= ">="
< "<"
<= "<="
== "=="
= "="

AND "AND"
OR "OR"

meta "META"

true "TRUE"
false "FALSE"

null "NULL"

' "SINGLE_QUOTE"

'[^']*' "STRING"

[a-z0-9_]* "NAME"

\( "("
\) ")"
\[ '['
\] ']'
:: '::'
, ','

[\t\n ]+ ;