HOST="localhost:8000" 
PASSWORD="2490-scorpion-include-wine-sister"
tar -cvf - ./wallpapers | age --encrypt -p -o - - <(echo -e "$PASSWORD") | curl -X POST -F token='13456' -F file=@- "http://$HOST/upload/"$(echo $PASSWORD | argon2 $HOST -r -i -t 3 -k 65536 -p 1)


HOST="localhost:8000" 
PASSWORD="2490-scorpion-include-wine-sister"

export PGP_PASSPHRASE="$PASSWORD" 
rage  --encrypt -p -o ~/test.age <(tar -cvf - ./wallpapers)

expect -c 'spawn bash -c "age --encrypt -p -o ~/test.age <(tar -cvf - ./wallpapers)"' -c "send -- '$PASSWORD\r'" -c "send -- '$PASSWORD\r'"

