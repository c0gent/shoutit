package com.cogciprocate.onesignalshoutit;

import android.content.Intent;
import android.support.v7.app.AppCompatActivity;
import android.os.Bundle;
//import android.support.v7.widget.LinearLayoutManager;
//import android.support.v7.widget.RecyclerView;
import android.view.View;
import android.widget.ArrayAdapter;
import android.widget.EditText;
import android.widget.ListView;
import android.widget.TextView;
import android.widget.Toast;

import com.android.volley.Request;
import com.android.volley.RequestQueue;
import com.android.volley.Response;
import com.android.volley.VolleyError;
import com.android.volley.VolleyLog;
import com.android.volley.toolbox.JsonObjectRequest;
import com.android.volley.toolbox.Volley;

import org.json.JSONException;
import org.json.JSONObject;

public class MainActivity extends AppCompatActivity {
    public static final String EXTRA_MESSAGE = "com.cogciprocate.onesignalshoutit.MESSAGE";
    private RequestQueue mRequestQueue;
    private EditText editText;
    private String messageText;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_main);

        mRequestQueue = Volley.newRequestQueue(this);
        editText = (EditText) findViewById(R.id.editText);
        messageText = "";
    }

    public void sendMessage(View view) {
        postShout();
    }

    private void postShout() {
//        String url =  "http://10.0.0.101:8080/shout";
        String url =  "http://cogciprocate.com:8080/shout";
//        Toast.makeText(MainActivity.this, "Posting message...", Toast.LENGTH_SHORT).show();
        JSONObject jsonBody = new JSONObject();
        messageText = editText.getText().toString();
        editText.setText("");
        try {
            jsonBody.put("message", messageText);
        } catch (JSONException e) {
            e.printStackTrace();
        }

        JsonObjectRequest req = new JsonObjectRequest(Request.Method.POST, url, jsonBody,
            new Response.Listener<JSONObject>() {
                @Override
                public void onResponse(JSONObject response) {
//                    try {
//                        String result = "The response is: " + response.getString("ip");
////                        String result = "Your message has been shouted!";
//                        Toast.makeText(MainActivity.this, result, Toast.LENGTH_SHORT).show();
//                    } catch (JSONException e) {
//                        e.printStackTrace();
//                    }
                    String msg = "\"" + messageText + "\" has been shouted!";
                    Toast.makeText(MainActivity.this, msg, Toast.LENGTH_SHORT).show();

                    String res = response.toString();
                    if (res != null && res.isEmpty()) {
                        String resp = "The response is: " + response.toString();
                        Toast.makeText(MainActivity.this, resp, Toast.LENGTH_SHORT).show();
                    }
                }
            }, new Response.ErrorListener() {
                @Override
                public void onErrorResponse(VolleyError error) {
                    VolleyLog.e("Error: ", error.getMessage());
//                    Toast.makeText(MainActivity.this, "Error: " +  error.getMessage(),
//                            Toast.LENGTH_LONG).show();
                }
            });

        mRequestQueue.add(req);
    }
}
